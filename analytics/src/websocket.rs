use std::net::SocketAddr;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tracing::{debug, info};

use crate::errors::WebSocketError;

#[derive(Debug)]
pub struct WebSocketServer {
    tcp_listener: TcpListener,
}

impl WebSocketServer {
    /// Creates a new WebSocket server.
    pub async fn new(host: String, port: u16) -> Result<Self, WebSocketError> {
        let tcp_listener = TcpListener::bind(format!("{}:{}", host, port))
            .await
            .map_err(|e| WebSocketError::Io(e.into()))?;

        Ok(Self { tcp_listener })
    }

    /// Starts the WebSocket server and listens for incoming connections.
    pub async fn start(
        &self,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
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
                    tokio::spawn(Self::accept_connection(stream, peer));
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutting down WebSocket server");
                    break Ok(());
                }
            }
        }
    }

    /// Accepts a new WebSocket connection.
    async fn accept_connection(
        stream: tokio::net::TcpStream,
        peer: SocketAddr,
    ) -> Result<(), WebSocketError> {
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .map_err(|e| WebSocketError::WebSocket(e.into()))?;

        info!("WebSocket connection established with {}", peer);
        // Here you can handle the WebSocket connection
        // For example, you can read messages from the WebSocket
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        while let Some(message) = ws_receiver.next().await {
            match message {
                Ok(msg) => {
                    debug!("Received message: {}", msg);
                    // Echo the message back to the client
                    if let Err(e) = ws_sender.send(msg).await {
                        debug!("Error sending message: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    debug!("Error receiving message: {}", e);
                    break;
                }
            }
        }

        info!("WebSocket connection closed with {}", peer);

        Ok(())
    }
}

/// Starts the WebSocket server and listens for incoming connections.
pub async fn server(
    host: impl Into<String>,
    port: u16,
    shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> Result<(), WebSocketError> {
    let server = WebSocketServer::new(host.into(), port).await?;
    server.start(shutdown_rx).await
}
