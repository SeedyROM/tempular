//! WebSocket Server Implementation with Pub/Sub Support
//!
//! This module provides a flexible WebSocket server implementation with built-in support
//! for publish/subscribe patterns. It features:
//! - Custom connection state management per client
//! - Asynchronous message handling
//! - Connection lifecycle management
//! - Generic message routing capabilities
//! - Built-in pub/sub functionality
//!
//! # Architecture
//! The implementation is based on a trait-based design where the core functionality
//! is defined by the `WebSocketHandler` trait, allowing for custom implementations
//! while providing a default pub/sub handler.

use anyhow::Result;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, net::SocketAddr, sync::Arc};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

use crate::{config::WebSocketConfig, errors::WebSocketError, messaging::MessageRouter};

/// Defines the behavior for handling WebSocket connections and messages.
///
/// This trait provides the core interface for implementing custom WebSocket server behavior.
/// Implementors can define their own connection state type and provide custom logic for
/// connection lifecycle events and message handling.
///
/// # Type Parameters
///
/// * `ConnectionState`: A type that represents the state maintained for each connection.
///   Must implement `Send + Default + 'static`.
///
/// # Examples
///
/// ```rust
/// # use async_trait::async_trait;
/// # use tokio_tungstenite::tungstenite::Message;
/// # use std::net::SocketAddr;
///
/// #[derive(Default)]
/// struct MyState {
///     message_count: usize,
/// }
///
/// #[derive(Clone)]
/// struct MyHandler;
///
/// #[async_trait]
/// impl WebSocketHandler for MyHandler {
///     type ConnectionState = MyState;
///     
///     async fn handle_message(
///         &self,
///         addr: SocketAddr,
///         state: &mut MyState,
///         message: Message,
///     ) -> Result<Vec<Message>, WebSocketError> {
///         state.message_count += 1;
///         Ok(vec![Message::Text(format!("Received message #{}", state.message_count))])
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait WebSocketHandler: Send + Sync + Clone + 'static {
    /// Represents the connection-specific state that persists across messages for a single WebSocket connection.
    ///
    /// This associated type must implement:
    /// - `Send`: Can be sent across thread boundaries
    /// - `Default`: Provides initial state for new connections
    /// - `'static`: Has no borrowed references
    type ConnectionState: Send + Default + 'static;

    /// Called when a new WebSocket connection is established with a client.
    ///
    /// This method allows implementations to perform any necessary setup or initialization
    /// for the new connection. The default implementation simply logs the connection.
    ///
    /// # Parameters
    /// * `_state` - Mutable reference to the connection state, initialized with `Default::default()`
    /// * `addr` - Socket address of the connected client
    ///
    /// # Returns
    /// * `Ok(())` if connection handling was successful
    /// * `Err(WebSocketError)` if an error occurred during connection handling
    async fn on_connect(
        &self,
        _state: &mut Self::ConnectionState,
        addr: SocketAddr,
    ) -> Result<(), WebSocketError> {
        debug!("New connection from {}", addr);
        Ok(())
    }

    /// Called when an existing WebSocket connection is terminated.
    ///
    /// This method allows implementations to perform any necessary cleanup or bookkeeping
    /// when a connection ends. The default implementation simply logs the disconnection.
    ///
    /// # Parameters
    /// * `_state` - Mutable reference to the connection state for the closing connection
    /// * `addr` - Socket address of the disconnecting client
    ///
    /// # Returns
    /// * `Ok(())` if disconnection handling was successful
    /// * `Err(WebSocketError)` if an error occurred during disconnect handling
    async fn on_disconnect(
        &self,
        _state: &mut Self::ConnectionState,
        addr: SocketAddr,
    ) -> Result<(), WebSocketError> {
        debug!("Connection closed from {}", addr);
        Ok(())
    }

    /// Processes an individual WebSocket message received from a client.
    ///
    /// This is the main message handling method that implementations must provide. It receives
    /// incoming messages, processes them according to the application's needs, and can respond
    /// with zero or more messages to be sent back to the client.
    ///
    /// # Parameters
    /// * `addr` - Socket address of the client that sent the message
    /// * `state` - Mutable reference to the connection-specific state
    /// * `message` - The received WebSocket message to be processed
    ///
    /// # Returns
    /// * `Ok(Vec<Message>)` - A vector of messages to be sent back to the client. Can be empty
    ///   if no response is needed
    /// * `Err(WebSocketError)` - If an error occurred during message processing
    async fn handle_message(
        &self,
        addr: SocketAddr,
        state: &mut Self::ConnectionState,
        message: Message,
    ) -> Result<Vec<Message>, WebSocketError>;
}

/// A WebSocket server implementation that can run any handler.
///
/// # Type Parameters
///
/// * `H`: A type that implements `WebSocketHandler`, defining how the server handles
///   connections and messages.
/// ```
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
        handler.on_connect(&mut connection_state, peer).await?;

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
        handler.on_disconnect(&mut connection_state, peer).await?;

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

/// A publish/subscribe handler implementation for the WebSocket server.
///
/// This handler provides functionality for:
/// - Topic subscription management
/// - Message broadcasting to subscribed clients
/// - Data point distribution
///
/// # Message Types
///
/// The handler supports three types of messages:
/// - Subscribe: Add subscriptions to specific topics
/// - Unsubscribe: Remove subscriptions from specific topics
/// - DataPoint: Send sensor data to subscribed clients
///
/// # Examples
///
/// ```rust
/// let handler = PubSubHandler;
/// let server = WebSocketServer::new("127.0.0.1".to_string(), 8080, handler).await?;
/// ```
#[derive(Clone, Debug)]
pub struct PubSubHandler;

/// Represents the types of messages that can be sent in the pub/sub system.
///
/// # Variants
///
/// * `Subscribe`: Request to subscribe to one or more topics
/// * `Unsubscribe`: Request to unsubscribe from one or more topics
/// * `DataPoint`: A data point message containing sensor information
///
/// # Examples
///
/// ```rust
/// let subscribe_msg = PubSubData::Subscribe {
///     topics: vec!["sensors".to_string(), "alerts".to_string()]
/// };
///
/// let data_point = PubSubData::DataPoint {
///     timestamp: 1234567890,
///     sensor_id: "temp_sensor_1".to_string(),
///     value: 23.5
/// };
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PubSubData {
    #[serde(rename = "subscribe")]
    Subscribe { topics: Vec<String> },
    #[serde(rename = "unsubscribe")]
    Unsubscribe { topics: Vec<String> },
    #[serde(rename = "data_point")]
    DataPoint {
        timestamp: u64,
        sensor_id: String,
        value: f64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PubSubMessage {
    #[serde(flatten)]
    data: PubSubData,
}

struct Connection {
    subscribed_topics: HashSet<String>,
}

/// State maintained for each connection in the pub/sub system.
///
/// This struct tracks per-connection information including:
/// - Message statistics
/// - Active topic subscriptions
/// - Reference to the message router
///
/// The state is automatically initialized when a new connection is established
/// and cleaned up when the connection is closed.
pub struct PubSubConnectionState {
    #[allow(unused)]
    router: MessageRouter<PubSubMessage>,
    messages_received: usize,
    connections: Arc<DashMap<SocketAddr, Connection>>,
}

impl Default for PubSubConnectionState {
    fn default() -> Self {
        Self {
            router: MessageRouter::new(),
            messages_received: 0,
            connections: Arc::new(DashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl WebSocketHandler for PubSubHandler {
    type ConnectionState = PubSubConnectionState;

    async fn on_connect(
        &self,
        state: &mut Self::ConnectionState,
        addr: SocketAddr,
    ) -> Result<(), WebSocketError> {
        info!("New connection from {}", addr);

        state.connections.insert(
            addr,
            Connection {
                subscribed_topics: HashSet::new(),
            },
        );

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

        let message = match message {
            Message::Text(text) => Message::Text(text),
            Message::Binary(bin) => Message::Binary(bin),
            _ => {
                warn!("Unsupported message type from {}", addr);
                return Ok(vec![]);
            }
        };

        let message_str = message
            .to_text()
            .map_err(|e| WebSocketError::WebSocket(e.into()))?;

        debug!("Received message from {}: {}", addr, message_str);

        let message: PubSubMessage = match serde_json::from_str(message_str) {
            Ok(msg) => msg,
            Err(e) => {
                warn!("Failed to parse message from {}: {}", addr, e);
                return Ok(vec![]);
            }
        };

        // Process the message
        match &message.data {
            PubSubData::Subscribe { topics } => {
                if let Some(mut connection) = state.connections.get_mut(&addr) {
                    for topic in topics {
                        connection.subscribed_topics.insert(topic.clone());
                    }
                }
            }
            PubSubData::Unsubscribe { topics } => {
                if let Some(mut connection) = state.connections.get_mut(&addr) {
                    for topic in topics {
                        connection.subscribed_topics.remove(topic);
                    }
                }
            }
            _ => {
                warn!("Unsupported message type from {}", addr);
            }
        }

        // Echo back the message
        let response =
            serde_json::to_string(&message).map_err(|e| WebSocketError::Serde(e.into()))?;

        Ok(vec![Message::Text(response.into())]) // Echo back the message
    }

    async fn on_disconnect(
        &self,
        state: &mut Self::ConnectionState,
        addr: SocketAddr,
    ) -> Result<(), WebSocketError> {
        info!("Connection closed from {}", addr);

        state.connections.remove(&addr);

        Ok(())
    }
}

/// Starts a specialized WebSocket server with pub/sub capabilities.
///
/// This function:
/// 1. Loads configuration from the given `WebSocketConfig`
/// 2. Creates a new PubSubHandler instance
/// 3. Initializes and starts the WebSocket server
///
/// # Arguments
///
/// * `config`: Configuration for the WebSocket server
/// * `shutdown_rx`: A broadcast receiver for graceful shutdown signaling
///
/// # Returns
///
/// * `Result<(), WebSocketError>`: Ok(()) on successful shutdown, or an error
///   if something goes wrong during server operation
///
/// # Examples
///
/// ```rust
/// let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
///
/// // Start the server
/// tokio::spawn(async move {
///     if let Err(e) = server(shutdown_rx).await {
///         eprintln!("Server error: {}", e);
///     }
/// });
///
/// // Later, initiate graceful shutdown
/// shutdown_tx.send(()).expect("Failed to send shutdown signal");
/// ```
pub async fn server(
    config: WebSocketConfig,
    shutdown_rx: broadcast::Receiver<()>,
) -> Result<(), WebSocketError> {
    let handler = PubSubHandler;
    let server = WebSocketServer::new(config.host, config.port, handler).await?;
    server.start(shutdown_rx).await
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
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

            async fn on_connect(
                &self,
                _state: &mut Self::ConnectionState,
                _addr: SocketAddr,
            ) -> Result<(), WebSocketError> {
                self.tracker
                    .connects
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }

            async fn on_disconnect(
                &self,
                _state: &mut Self::ConnectionState,
                _addr: SocketAddr,
            ) -> Result<(), WebSocketError> {
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

            async fn on_connect(
                &self,
                _state: &mut Self::ConnectionState,
                _addr: SocketAddr,
            ) -> Result<(), WebSocketError> {
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

            async fn on_disconnect(
                &self,
                _state: &mut Self::ConnectionState,
                _addr: SocketAddr,
            ) -> Result<(), WebSocketError> {
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

    async fn setup_server() -> (
        SocketAddr,
        broadcast::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let handler = PubSubHandler;
        let server = WebSocketServer::new("127.0.0.1".into(), 0, handler)
            .await
            .unwrap();
        let addr = server.tcp_listener.local_addr().unwrap();

        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let handle = tokio::spawn(async move {
            let _ = server.start(shutdown_rx).await;
        });

        (addr, shutdown_tx, handle)
    }

    #[tokio::test]
    async fn test_subscribe_unsubscribe() {
        let (addr, shutdown_tx, handle) = setup_server().await;

        let (mut ws_stream, _) = tokio_tungstenite::connect_async(format!("ws://{}", addr))
            .await
            .expect("Failed to connect");

        let subscribe_msg = json!({
            "subscribe": {
                "topics": ["test-topic"]
            }
        })
        .to_string();

        ws_stream
            .send(Message::Text(subscribe_msg.into()))
            .await
            .unwrap();

        if let Some(Ok(response)) =
            tokio::time::timeout(Duration::from_secs_f32(0.1), ws_stream.next())
                .await
                .unwrap()
        {
            let resp_text = response.to_text().unwrap();
            let parsed: PubSubMessage = serde_json::from_str(resp_text).unwrap();
            match parsed.data {
                PubSubData::Subscribe { topics } => {
                    assert_eq!(topics, vec!["test-topic"]);
                }
                _ => panic!("Expected subscribe response"),
            }
        }

        let unsubscribe_msg = json!({
            "unsubscribe": {
                "topics": ["test-topic"]
            }
        })
        .to_string();

        ws_stream
            .send(Message::Text(unsubscribe_msg.into()))
            .await
            .unwrap();

        drop(ws_stream);
        shutdown_tx.send(()).unwrap();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_data_point_handling() {
        let (addr, shutdown_tx, handle) = setup_server().await;

        let (mut ws_stream, _) = tokio_tungstenite::connect_async(format!("ws://{}", addr))
            .await
            .expect("Failed to connect");

        let data_point_msg = json!({
            "data_point": {
                "timestamp": 1234567890,
                "sensor_id": "sensor1",
                "value": 42.5
            }
        })
        .to_string();

        ws_stream
            .send(Message::Text(data_point_msg.into()))
            .await
            .unwrap();

        if let Some(Ok(response)) =
            tokio::time::timeout(Duration::from_secs_f32(0.1), ws_stream.next())
                .await
                .unwrap()
        {
            let resp_text = response.to_text().unwrap();
            let parsed: PubSubMessage = serde_json::from_str(resp_text).unwrap();
            match parsed.data {
                PubSubData::DataPoint {
                    timestamp,
                    sensor_id,
                    value,
                } => {
                    assert_eq!(timestamp, 1234567890);
                    assert_eq!(sensor_id, "sensor1");
                    assert_eq!(value, 42.5);
                }
                _ => panic!("Expected data point response"),
            }
        }

        drop(ws_stream);
        shutdown_tx.send(()).unwrap();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_multiple_topics() {
        let (addr, shutdown_tx, handle) = setup_server().await;

        let (mut ws_stream, _) = tokio_tungstenite::connect_async(format!("ws://{}", addr))
            .await
            .expect("Failed to connect");

        let subscribe_msg = json!({
            "subscribe": {
                "topics": ["topic1", "topic2", "topic3"]
            }
        })
        .to_string();

        ws_stream
            .send(Message::Text(subscribe_msg.into()))
            .await
            .unwrap();

        if let Some(Ok(response)) =
            tokio::time::timeout(Duration::from_secs_f32(0.1), ws_stream.next())
                .await
                .unwrap()
        {
            let resp_text = response.to_text().unwrap();
            let parsed: PubSubMessage = serde_json::from_str(resp_text).unwrap();
            match parsed.data {
                PubSubData::Subscribe { topics } => {
                    assert_eq!(topics.len(), 3);
                    assert!(topics.contains(&"topic1".to_string()));
                    assert!(topics.contains(&"topic2".to_string()));
                    assert!(topics.contains(&"topic3".to_string()));
                }
                _ => panic!("Expected subscribe response"),
            }
        }

        drop(ws_stream);
        shutdown_tx.send(()).unwrap();
        let _ = handle.await;
    }
}
