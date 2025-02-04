//! Application configuration management module
//!
//! This module provides a configuration system for the application, supporting both file-based
//! and environment variable-based configuration. It handles settings for RabbitMQ and WebSocket
//! services with sensible defaults.
//!
//! # Environment Variables
//!
//! Configuration can be set using environment variables with the `APP_` prefix:
//!
//! ```text
//! # RabbitMQ Configuration
//! APP_RABBITMQ_HOST=localhost
//! APP_RABBITMQ_PORT=5672
//! APP_RABBITMQ_USERNAME=guest
//! APP_RABBITMQ_PASSWORD=guest
//! APP_RABBITMQ_QUEUE_NAME=analytics
//!
//! # WebSocket Configuration
//! APP_WEBSOCKET_HOST=127.0.0.1
//! APP_WEBSOCKET_PORT=8080
//! ```
//!
//! # Configuration File
//!
//! Alternatively, settings can be specified in a `config` file (format determined by extension):
//!
//! ```toml
//! [rabbitmq]
//! host = "localhost"
//! port = "5672"
//! username = "guest"
//! password = "guest"
//! queue_name = "analytics"
//!
//! [websocket]
//! host = "127.0.0.1"
//! port = 8080
//! ```

use config::{Config, ConfigError, Environment};
use serde::Deserialize;
use tracing::debug;

/// Configuration for RabbitMQ connection settings
#[derive(Debug, Deserialize, Clone)]
pub struct RabbitMQConfig {
    /// RabbitMQ server hostname (default: "localhost")
    #[serde(default = "default_host")]
    pub host: String,

    /// RabbitMQ server port (default: "5672")
    #[serde(default = "default_port")]
    pub port: String,

    /// RabbitMQ authentication username (default: "guest")
    #[serde(default = "default_username")]
    pub username: String,

    /// RabbitMQ authentication password (default: "guest")
    #[serde(default = "default_password")]
    pub password: String,

    /// Name of the RabbitMQ queue to use (default: "analytics")
    #[serde(default = "default_queue_name")]
    pub queue_name: String,
}

/// Configuration for WebSocket server settings
#[derive(Debug, Deserialize, Clone)]
pub struct WebSocketConfig {
    /// WebSocket server hostname (default: "127.0.0.1")
    #[serde(default = "default_ws_host")]
    pub host: String,

    /// WebSocket server port (default: 8080)
    #[serde(default = "default_ws_port")]
    pub port: u16,
}

/// Main application configuration structure
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    /// RabbitMQ-specific configuration
    #[serde(default)]
    pub rabbitmq: RabbitMQConfig,

    /// WebSocket-specific configuration
    #[serde(default)]
    pub websocket: WebSocketConfig,
}

// Default implementations
impl Default for RabbitMQConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            username: default_username(),
            password: default_password(),
            queue_name: default_queue_name(),
        }
    }
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            host: default_ws_host(),
            port: default_ws_port(),
        }
    }
}

// Default value functions
fn default_host() -> String {
    "localhost".to_string()
}
fn default_port() -> String {
    "5672".to_string()
}
fn default_username() -> String {
    "guest".to_string()
}
fn default_password() -> String {
    "guest".to_string()
}
fn default_queue_name() -> String {
    "analytics".to_string()
}
fn default_ws_host() -> String {
    "127.0.0.1".to_string()
}
fn default_ws_port() -> u16 {
    8080
}

impl RabbitMQConfig {
    /// Generates a RabbitMQ connection URL from the configuration
    ///
    /// # Returns
    ///
    /// Returns a formatted AMQP URL string in the format:
    /// `amqp://username:password@host:port`
    pub fn url(&self) -> String {
        format!(
            "amqp://{}:{}@{}:{}",
            self.username, self.password, self.host, self.port
        )
    }
}

impl AppConfig {
    /// Creates a new AppConfig instance by loading configuration from environment variables
    /// and an optional configuration file
    ///
    /// # Configuration Sources
    ///
    /// 1. File source: Attempts to load from a file named "config" (optional)
    /// 2. Environment variables: Loads from environment variables with prefix "APP_"
    ///
    /// Environment variables take precedence over file configuration.
    ///
    /// # Returns
    ///
    /// Returns a Result containing either the populated AppConfig instance or a ConfigError
    ///
    /// # Example
    ///
    /// ```rust
    /// use crate::config::AppConfig;
    ///
    /// match AppConfig::new() {
    ///     Ok(config) => println!("Configuration loaded successfully"),
    ///     Err(e) => eprintln!("Failed to load configuration: {}", e),
    /// }
    /// ```
    pub fn new() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(config::File::with_name("config").required(false))
            .add_source(
                Environment::with_prefix("APP")
                    .separator("_")
                    .try_parsing(true),
            )
            .build()?;

        let app_config: AppConfig = config.try_deserialize()?;

        debug!("Loaded configuration: {:#?}", app_config);

        Ok(app_config)
    }
}
