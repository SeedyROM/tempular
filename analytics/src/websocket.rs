use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info};

use crate::errors::WebSocketError;

// Core trait that defines WebSocket server behavior
#[async_trait::async_trait]
pub trait WebSocketHandler: Send + Sync + Clone {
    async fn handle_connection(
        &self,
        addr: SocketAddr,
        messages: Vec<Message>,
    ) -> Result<Vec<Message>>;
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
        handler: &H,
        stream: tokio::net::TcpStream,
        peer: SocketAddr,
    ) -> Result<(), WebSocketError> {
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .map_err(|e| WebSocketError::WebSocket(e.into()))?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        let mut messages = Vec::new();

        while let Some(Ok(msg)) = ws_receiver.next().await {
            messages.push(msg);
        }

        if let Ok(responses) = handler.handle_connection(peer, messages).await {
            for response in responses {
                let _ = ws_sender.send(response).await;
            }
        }

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
                    // Handle the connection directly without spawning
                    if let Err(e) = Self::handle_tcp_stream(&self.handler, stream, peer).await {
                        debug!("Error handling connection: {}", e);
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutting down WebSocket server");
                    break Ok(());
                }
            }
        }
    }
}

// Real implementation of the handler
#[derive(Clone, Debug)]
pub struct EchoHandler;

#[async_trait::async_trait]
impl WebSocketHandler for EchoHandler {
    async fn handle_connection(
        &self,
        addr: SocketAddr,
        messages: Vec<Message>,
    ) -> Result<Vec<Message>> {
        debug!("Handling messages from {}", addr);
        Ok(messages) // Echo back all messages
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

    // Mock implementation for testing
    #[derive(Clone, Debug)]
    pub struct MockHandler {
        pub responses: Vec<Message>,
    }

    #[async_trait::async_trait]
    impl WebSocketHandler for MockHandler {
        async fn handle_connection(
            &self,
            _addr: SocketAddr,
            _messages: Vec<Message>,
        ) -> Result<Vec<Message>> {
            Ok(self.responses.clone())
        }
    }

    // Mock handler that echoes received messages
    #[derive(Clone, Debug)]
    struct EchoMockHandler;

    #[async_trait::async_trait]
    impl WebSocketHandler for EchoMockHandler {
        async fn handle_connection(
            &self,
            _addr: SocketAddr,
            messages: Vec<Message>,
        ) -> Result<Vec<Message>> {
            Ok(messages)
        }
    }

    // Mock handler that simulates errors
    #[derive(Clone, Debug)]
    struct ErrorMockHandler;

    #[async_trait::async_trait]
    impl WebSocketHandler for ErrorMockHandler {
        async fn handle_connection(
            &self,
            _addr: SocketAddr,
            _messages: Vec<Message>,
        ) -> Result<Vec<Message>> {
            Err(anyhow::anyhow!("Simulated handler error"))
        }
    }

    #[tokio::test]
    async fn test_mock_handler_basic() {
        let test_messages = vec![Message::Text("test1".into()), Message::Text("test2".into())];
        let mock_handler = MockHandler {
            responses: test_messages.clone(),
        };
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let input_messages = vec![Message::Text("input".into())];

        let result = mock_handler
            .handle_connection(addr, input_messages)
            .await
            .unwrap();
        assert_eq!(result, test_messages);
    }

    #[tokio::test]
    async fn test_echo_handler() {
        let messages = vec![
            Message::Text("Hello".into()),
            Message::Binary(vec![1, 2, 3].into()),
            Message::Ping(vec![].into()),
        ];
        let handler = EchoHandler;
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();

        let result = handler
            .handle_connection(addr, messages.clone())
            .await
            .unwrap();
        assert_eq!(result, messages);
    }

    #[tokio::test]
    async fn test_server_creation() -> Result<()> {
        let handler = EchoHandler;
        let server = WebSocketServer::new("127.0.0.1".into(), 0, handler).await?;

        let addr = server.tcp_listener.local_addr()?;
        assert!(addr.port() > 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_server_shutdown() -> Result<()> {
        let handler = EchoHandler;
        let server = WebSocketServer::new("127.0.0.1".into(), 0, handler).await?;

        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        // Spawn server task
        let server_handle = tokio::spawn(async move { server.start(shutdown_rx).await });

        // Send shutdown signal
        shutdown_tx
            .send(())
            .expect("Failed to send shutdown signal");

        // Wait for server to shutdown
        let result = tokio::time::timeout(Duration::from_secs(1), server_handle).await??;
        assert!(result.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn test_server_multiple_handlers() -> Result<()> {
        let handler = MockHandler {
            responses: vec![Message::Text("response".into())],
        };

        let server = WebSocketServer::new("127.0.0.1".into(), 0, handler.clone()).await?;
        let addr = server.tcp_listener.local_addr()?;

        // Test that handler can be cloned and used multiple times
        let result1 = handler
            .handle_connection(addr, vec![Message::Text("test1".into())])
            .await?;
        let result2 = handler
            .handle_connection(addr, vec![Message::Text("test2".into())])
            .await?;

        assert_eq!(result1, result2);
        Ok(())
    }

    #[tokio::test]
    async fn test_error_handler() {
        let handler = ErrorMockHandler;
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let messages = vec![Message::Text("test".into())];

        let result = handler.handle_connection(addr, messages).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Simulated handler error");
    }

    #[tokio::test]
    async fn test_different_message_types() -> Result<()> {
        let messages = vec![
            Message::Text("text message".into()),
            Message::Binary(vec![1, 2, 3].into()),
            Message::Ping(vec![4, 5, 6].into()),
            Message::Pong(vec![7, 8, 9].into()),
            Message::Close(None),
        ];

        let handler = EchoMockHandler;
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();

        let result = handler.handle_connection(addr, messages.clone()).await?;
        assert_eq!(result, messages);
        Ok(())
    }
}
