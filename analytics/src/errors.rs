use thiserror::Error;

#[allow(unused)]
#[derive(Debug, Error)]
pub enum RabbitMQError {
    #[error("Failed to connect to RabbitMQ: {0}")]
    ConnectionError(#[from] lapin::Error),

    #[error("Failed to parse message: {0}")]
    MessageParseError(String),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("DotEnv error: {0}")]
    DotEnvError(#[from] dotenv::Error),
    #[error("Environment variable not found: {0}")]
    EnvVarError(#[from] std::env::VarError),
}

#[derive(Debug, Error)]
pub enum WebSocketError {
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
