use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

use crate::errors::WebSocketError;

// Enhanced trait that defines WebSocket server behavior with connection state
#[async_trait::async_trait]
pub trait WebSocketHandler: Send + Sync + Clone + 'static {
    /// Custom connection state that can be maintained across messages
    type ConnectionState: Send + Default + 'static;

    /// Called when a new connection is established
    async fn on_connect(&self, addr: SocketAddr) -> Result<(), WebSocketError> {
        debug!("New connection from {}", addr);
        Ok(())
    }

    /// Called when a connection is closed
    async fn on_disconnect(&self, addr: SocketAddr) -> Result<(), WebSocketError> {
        debug!("Connection closed from {}", addr);
        Ok(())
    }

    /// Handle an individual message
    async fn handle_message(
        &self,
        addr: SocketAddr,
        state: &mut Self::ConnectionState,
        message: Message,
    ) -> Result<Vec<Message>, WebSocketError>;
}

// Real implementation that uses actual TCP/WebSocket
#[derive(Debug)]
pub struct WebSocketServer<H: WebSocketHandler> {
    handler: H,
    tcp_listener: tokio::net::TcpListener,
}

impl<H: WebSocketHandler> WebSocketServer<H> {
    pub async fn new(host: String, port: u16, handler: H) -> Result<Self, WebSocketError> {
        let tcp_listener = tokio::net::TcpListener::bind(format!("{}:{}", host, port))
            .await
            .map_err(|e| WebSocketError::Io(e.into()))?;

        Ok(Self {
            tcp_listener,
            handler,
        })
    }

    async fn handle_tcp_stream(
        handler: H,
        stream: tokio::net::TcpStream,
        peer: SocketAddr,
    ) -> Result<(), WebSocketError> {
        // Accept the WebSocket connection
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .map_err(|e| WebSocketError::WebSocket(e.into()))?;

        // Split the WebSocket stream
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // Initialize connection state
        let mut connection_state = H::ConnectionState::default();

        // Notify handler of new connection
        handler.on_connect(peer).await?;

        // Process messages as they arrive
        while let Some(msg_result) = ws_receiver.next().await {
            match msg_result {
                Ok(message) => {
                    // Handle the message
                    match handler
                        .handle_message(peer, &mut connection_state, message)
                        .await
                    {
                        Ok(responses) => {
                            // Send responses
                            for response in responses {
                                if let Err(e) = ws_sender.send(response).await {
                                    error!("Failed to send message to {}: {}", peer, e);
                                    return Err(WebSocketError::WebSocket(e.into()));
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Error handling message from {}: {}", peer, e);
                            // Don't break the connection on handler errors
                        }
                    }
                }
                Err(e) => {
                    error!("WebSocket error from {}: {}", peer, e);
                    break;
                }
            }
        }

        // Notify handler of disconnection
        handler.on_disconnect(peer).await?;

        Ok(())
    }

    pub async fn start(
        &self,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<(), WebSocketError> {
        let addr = self
            .tcp_listener
            .local_addr()
            .map_err(|e| WebSocketError::Io(e.into()))?;
        info!("WebSocket server listening on {}", addr);

        loop {
            tokio::select! {
                Ok((stream, peer)) = self.tcp_listener.accept() => {
                    info!("New connection from {}", peer);
                    let handler = self.handler.clone();

                    // Spawn a new task for each connection
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_tcp_stream(handler, stream, peer).await {
                            error!("Error handling connection from {}: {}", peer, e);
                        }
                    });
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutting down WebSocket server");
                    break Ok(());
                }
            }
        }
    }
}

// Enhanced echo handler implementation
#[derive(Clone, Debug)]
pub struct EchoHandler;

// Simple connection state example
#[derive(Default)]
pub struct EchoConnectionState {
    messages_received: usize,
}

#[async_trait::async_trait]
impl WebSocketHandler for EchoHandler {
    type ConnectionState = EchoConnectionState;

    async fn on_connect(&self, addr: SocketAddr) -> Result<(), WebSocketError> {
        info!("New echo connection from {}", addr);
        Ok(())
    }

    async fn handle_message(
        &self,
        addr: SocketAddr,
        state: &mut Self::ConnectionState,
        message: Message,
    ) -> Result<Vec<Message>, WebSocketError> {
        state.messages_received += 1;
        debug!("Handling message {} from {}", state.messages_received, addr);
        Ok(vec![message]) // Echo back the message
    }

    async fn on_disconnect(&self, addr: SocketAddr) -> Result<(), WebSocketError> {
        info!("Echo connection closed from {}", addr);
        Ok(())
    }
}

/// Starts the WebSocket server with default configuration
pub async fn server(shutdown_rx: broadcast::Receiver<()>) -> Result<(), WebSocketError> {
    use crate::{config::WebSocketConfig, traits::FromEnv};

    let WebSocketConfig { host, port } = WebSocketConfig::from_env();
    let handler = EchoHandler;

    let server = WebSocketServer::new(host.into(), port, handler).await?;
    server.start(shutdown_rx).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Enhanced mock handler with connection state
    #[derive(Clone, Debug)]
    pub struct MockHandler {
        pub responses: Vec<Message>,
    }

    #[derive(Default)]
    pub struct MockConnectionState {
        messages_processed: usize,
    }

    #[async_trait::async_trait]
    impl WebSocketHandler for MockHandler {
        type ConnectionState = MockConnectionState;

        async fn handle_message(
            &self,
            _addr: SocketAddr,
            state: &mut Self::ConnectionState,
            _message: Message,
        ) -> Result<Vec<Message>, WebSocketError> {
            state.messages_processed += 1;
            Ok(self.responses.clone())
        }
    }

    #[tokio::test]
    async fn test_mock_handler_with_state() {
        let test_messages = vec![Message::Text("test1".into()), Message::Text("test2".into())];
        let mock_handler = MockHandler {
            responses: test_messages.clone(),
        };
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let mut state = MockConnectionState::default();

        let result = mock_handler
            .handle_message(addr, &mut state, Message::Text("input".into()))
            .await
            .unwrap();

        assert_eq!(result, test_messages);
        assert_eq!(state.messages_processed, 1);
    }

    // Test connection lifecycle
    #[tokio::test]
    async fn test_connection_lifecycle() -> Result<()> {
        #[derive(Debug, Default)]
        struct LifecycleTracker {
            connects: std::sync::atomic::AtomicUsize,
            disconnects: std::sync::atomic::AtomicUsize,
        }

        #[derive(Clone, Debug)]
        struct LifecycleHandler {
            tracker: std::sync::Arc<LifecycleTracker>,
        }

        #[derive(Default)]
        struct LifecycleState;

        #[async_trait::async_trait]
        impl WebSocketHandler for LifecycleHandler {
            type ConnectionState = LifecycleState;

            async fn on_connect(&self, _addr: SocketAddr) -> Result<(), WebSocketError> {
                self.tracker
                    .connects
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }

            async fn on_disconnect(&self, _addr: SocketAddr) -> Result<(), WebSocketError> {
                self.tracker
                    .disconnects
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }

            async fn handle_message(
                &self,
                _addr: SocketAddr,
                _state: &mut Self::ConnectionState,
                message: Message,
            ) -> Result<Vec<Message>, WebSocketError> {
                Ok(vec![message])
            }
        }

        let tracker = std::sync::Arc::new(LifecycleTracker::default());
        let handler = LifecycleHandler {
            tracker: tracker.clone(),
        };

        let server = WebSocketServer::new("127.0.0.1".into(), 0, handler).await?;
        let addr = server.tcp_listener.local_addr()?;

        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        // Start server in background
        let server_handle = tokio::spawn(async move { server.start(shutdown_rx).await });

        // Connect a client
        let (ws_stream, _) = tokio_tungstenite::connect_async(format!("ws://{}", addr))
            .await
            .expect("Failed to connect");

        // Close the connection
        drop(ws_stream);

        // Wait a bit for disconnect to be processed
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Shutdown server
        shutdown_tx
            .send(())
            .expect("Failed to send shutdown signal");
        server_handle.await??;

        assert_eq!(
            tracker.connects.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            tracker
                .disconnects
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );

        Ok(())
    }

    // Test error handling
    #[tokio::test]
    async fn test_error_handling() -> Result<()> {
        #[derive(Clone, Debug)]
        struct ErrorHandler {
            should_fail: bool,
        }

        #[derive(Default)]
        struct ErrorState;

        #[async_trait::async_trait]
        impl WebSocketHandler for ErrorHandler {
            type ConnectionState = ErrorState;

            async fn handle_message(
                &self,
                _addr: SocketAddr,
                _state: &mut Self::ConnectionState,
                message: Message,
            ) -> Result<Vec<Message>, WebSocketError> {
                if self.should_fail {
                    Err(WebSocketError::Custom("Simulated handler error".into()))
                } else {
                    Ok(vec![message])
                }
            }
        }

        let handler = ErrorHandler { should_fail: true };
        let server = WebSocketServer::new("127.0.0.1".into(), 0, handler).await?;
        let addr = server.tcp_listener.local_addr()?;

        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        // Start server in background
        let server_handle = tokio::spawn(async move { server.start(shutdown_rx).await });

        // Connect a client
        let (ws_stream, _) = tokio_tungstenite::connect_async(format!("ws://{}", addr))
            .await
            .expect("Failed to connect");
        let (mut write, _read) = ws_stream.split();

        // Send a message that should trigger an error
        let send_result = write.send(Message::Text("test".into())).await;
        assert!(
            send_result.is_ok(),
            "Message send should succeed even if handler fails"
        );

        // Shutdown server
        shutdown_tx
            .send(())
            .expect("Failed to send shutdown signal");
        server_handle.await??;

        Ok(())
    }

    // Test concurrent connections
    #[tokio::test]
    async fn test_concurrent_connections() -> Result<()> {
        #[derive(Debug, Default)]
        struct ConcurrentTracker {
            active_connections: std::sync::atomic::AtomicUsize,
            max_connections: std::sync::atomic::AtomicUsize,
        }

        #[derive(Clone, Debug)]
        struct ConcurrentHandler {
            tracker: std::sync::Arc<ConcurrentTracker>,
        }

        #[derive(Default)]
        struct ConcurrentState;

        #[async_trait::async_trait]
        impl WebSocketHandler for ConcurrentHandler {
            type ConnectionState = ConcurrentState;

            async fn on_connect(&self, _addr: SocketAddr) -> Result<(), WebSocketError> {
                let active = self
                    .tracker
                    .active_connections
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;
                let mut max = self
                    .tracker
                    .max_connections
                    .load(std::sync::atomic::Ordering::SeqCst);
                while active > max {
                    self.tracker
                        .max_connections
                        .compare_exchange(
                            max,
                            active,
                            std::sync::atomic::Ordering::SeqCst,
                            std::sync::atomic::Ordering::SeqCst,
                        )
                        .unwrap_or_else(|x| {
                            max = x;

                            max
                        });
                }
                Ok(())
            }

            async fn on_disconnect(&self, _addr: SocketAddr) -> Result<(), WebSocketError> {
                self.tracker
                    .active_connections
                    .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }

            async fn handle_message(
                &self,
                _addr: SocketAddr,
                _state: &mut Self::ConnectionState,
                message: Message,
            ) -> Result<Vec<Message>, WebSocketError> {
                Ok(vec![message])
            }
        }

        let tracker = std::sync::Arc::new(ConcurrentTracker::default());
        let handler = ConcurrentHandler {
            tracker: tracker.clone(),
        };

        let server = WebSocketServer::new("127.0.0.1".into(), 0, handler).await?;
        let addr = server.tcp_listener.local_addr()?;

        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        // Start server in background
        let server_handle = tokio::spawn(async move { server.start(shutdown_rx).await });

        // Create multiple concurrent connections
        let num_connections = 5;
        let mut connections = Vec::new();

        for _ in 0..num_connections {
            let (ws_stream, _) = tokio_tungstenite::connect_async(format!("ws://{}", addr))
                .await
                .expect("Failed to connect");
            connections.push(ws_stream);
        }

        // Wait a bit to ensure all connections are processed
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Verify we reached expected concurrent connections
        assert_eq!(
            tracker
                .active_connections
                .load(std::sync::atomic::Ordering::SeqCst),
            num_connections
        );
        assert_eq!(
            tracker
                .max_connections
                .load(std::sync::atomic::Ordering::SeqCst),
            num_connections
        );

        // Close all connections
        connections.clear();

        // Wait for disconnects to be processed
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Verify all connections were closed
        assert_eq!(
            tracker
                .active_connections
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );

        // Shutdown server
        shutdown_tx
            .send(())
            .expect("Failed to send shutdown signal");
        server_handle.await??;

        Ok(())
    }

    // Test connection state
    #[tokio::test]
    async fn test_connection_state() -> Result<()> {
        #[derive(Default)]
        struct TestState {
            message_count: usize,
        }

        #[derive(Clone, Debug)]
        struct StateHandler;

        #[async_trait::async_trait]
        impl WebSocketHandler for StateHandler {
            type ConnectionState = TestState;

            async fn handle_message(
                &self,
                _addr: SocketAddr,
                state: &mut Self::ConnectionState,
                _message: Message,
            ) -> Result<Vec<Message>, WebSocketError> {
                state.message_count += 1;
                Ok(vec![Message::Text(
                    format!("Message count: {}", state.message_count).into(),
                )])
            }
        }

        let handler = StateHandler;
        let server = WebSocketServer::new("127.0.0.1".into(), 0, handler).await?;
        let addr = server.tcp_listener.local_addr()?;

        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        // Start server in background
        let server_handle = tokio::spawn(async move { server.start(shutdown_rx).await });

        // Connect a client
        let (ws_stream, _) = tokio_tungstenite::connect_async(format!("ws://{}", addr))
            .await
            .expect("Failed to connect");
        let (mut write, mut read) = ws_stream.split();

        // Send multiple messages
        for _ in 0..3 {
            write.send(Message::Text("test".into())).await?;
            if let Some(Ok(Message::Text(response))) = read.next().await {
                assert!(response.starts_with("Message count: "));
            } else {
                panic!("Expected text message response");
            }
        }

        // Shutdown server
        shutdown_tx
            .send(())
            .expect("Failed to send shutdown signal");
        server_handle.await??;

        Ok(())
    }
}
