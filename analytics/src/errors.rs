use thiserror::Error;

/// Errors that can occur during RabbitMQ operations.
///
/// This enum encompasses errors from both the RabbitMQ connection layer and
/// message processing operations.
#[allow(unused)]
#[derive(Debug, Error)]
pub enum RabbitMQError {
    /// Represents failures in establishing or maintaining a RabbitMQ connection.
    ///
    /// This variant wraps errors from the lapin RabbitMQ client library and is automatically
    /// converted from `lapin::Error` through the `From` trait.
    #[error("Failed to connect to RabbitMQ: {0}")]
    ConnectionError(#[from] lapin::Error),

    /// Indicates a failure to parse or process a RabbitMQ message.
    ///
    /// This variant is used when a message's content cannot be properly decoded
    /// or processed according to the application's requirements.
    #[error("Failed to parse message: {0}")]
    MessageParseError(String),
}

/// Errors that can occur during configuration loading and processing.
///
/// This enum handles errors related to environment variable loading and parsing,
/// including both .env file operations and accessing individual environment variables.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Represents errors that occur while loading or parsing the .env file.
    ///
    /// This variant is automatically converted from `dotenv::Error` through the
    /// `From` trait implementation.
    #[error("DotEnv error: {0}")]
    DotEnvError(#[from] dotenv::Error),

    /// Indicates that a required environment variable was not found or could not be accessed.
    ///
    /// This variant is automatically converted from `std::env::VarError` through the
    /// `From` trait implementation and covers both missing variables and invalid Unicode.
    #[error("Environment variable not found: {0}")]
    EnvVarError(#[from] std::env::VarError),
}

/// Errors that can occur during WebSocket operations.
///
/// This enum encompasses various error types that might occur during WebSocket
/// communication, including connection handling, I/O operations, serialization,
/// and custom application-specific errors.
#[derive(Debug, Error)]
pub enum WebSocketError {
    /// Represents WebSocket protocol-specific errors.
    ///
    /// This variant wraps errors from the tungstenite WebSocket implementation
    /// and is automatically converted through the `From` trait.
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    /// Represents low-level I/O errors that may occur during WebSocket operations.
    ///
    /// This variant wraps standard I/O errors and is automatically converted
    /// from `std::io::Error` through the `From` trait.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Represents application-specific custom errors.
    ///
    /// This variant allows for custom error messages to be propagated through
    /// the WebSocket error handling system.
    #[allow(unused)]
    #[error("Custom error: {0}")]
    Custom(String),

    /// Represents JSON serialization or deserialization errors.
    ///
    /// This variant wraps serde_json errors that may occur when processing
    /// JSON WebSocket messages and is automatically converted through the `From` trait.
    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Errors that can occur during environment setup and configuration.
///
/// This enum specifically handles errors related to the presence and validity
/// of environment configuration files.
#[derive(Debug, Error)]
pub enum EnvironmentError {
    /// Indicates that the required .env file could not be found in the expected location.
    ///
    /// This error occurs when the application requires a .env file for configuration
    /// but cannot locate it in the current directory or specified path.
    #[error(".env file not found")]
    DotEnvNotFound,
}
