use std::env;
use tracing::warn;

use crate::traits::FromEnv;

/// Configuration for connecting to a RabbitMQ server.
///
/// This struct holds all necessary connection parameters for establishing a RabbitMQ
/// connection and specifying the queue to use. It can be created either with default
/// values or from environment variables.
///
/// # Environment Variables
/// - `RABBITMQ_HOST`: The hostname of the RabbitMQ server (default: "localhost")
/// - `RABBITMQ_PORT`: The port number for the RabbitMQ server (default: "5672")
/// - `RABBITMQ_USER`: Username for authentication (default: "guest")
/// - `RABBITMQ_PASS`: Password for authentication (default: "guest")
/// - `RABBITMQ_PRODUCER_QUEUE`: Name of the queue to use (default: "sensor_data")
#[derive(Debug, Clone)]
pub struct RabbitMQConfig {
    /// The hostname of the RabbitMQ server
    pub host: String,
    /// The port number as a string
    pub port: String,
    /// Username for RabbitMQ authentication
    pub username: String,
    /// Password for RabbitMQ authentication
    pub password: String,
    /// Name of the queue to publish/consume messages from
    pub queue_name: String,
}

impl Default for RabbitMQConfig {
    /// Creates a default configuration with localhost settings and guest credentials.
    ///
    /// # Returns
    /// A new `RabbitMQConfig` instance with default values suitable for local development.
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: "5672".to_string(),
            username: "guest".to_string(),
            password: "guest".to_string(),
            queue_name: "sensor_data".to_string(),
        }
    }
}

impl RabbitMQConfig {
    /// Constructs the AMQP URL string from the configuration values.
    ///
    /// # Returns
    /// A formatted AMQP URL string in the format: "amqp://username:password@host:port"
    pub fn url(&self) -> String {
        format!(
            "amqp://{}:{}@{}:{}",
            self.username, self.password, self.host, self.port
        )
    }
}

impl FromEnv for RabbitMQConfig {
    /// Creates a new configuration instance from environment variables.
    ///
    /// If any environment variable is not set, falls back to the default value
    /// and logs a warning message using the `tracing` crate.
    ///
    /// # Returns
    /// A new `RabbitMQConfig` instance with values from environment variables or defaults
    fn from_env() -> Self {
        let default = Self::default();

        Self {
            host: env::var("RABBITMQ_HOST").unwrap_or_else(|_| {
                warn!("RABBITMQ_HOST not set, defaulting to {}", default.host);
                default.host
            }),
            port: env::var("RABBITMQ_PORT").unwrap_or_else(|_| {
                warn!("RABBITMQ_PORT not set, defaulting to {}", default.port);
                default.port
            }),
            username: env::var("RABBITMQ_USER").unwrap_or_else(|_| {
                warn!("RABBITMQ_USER not set, defaulting to {}", default.username);
                default.username
            }),
            password: env::var("RABBITMQ_PASS").unwrap_or_else(|_| {
                warn!("RABBITMQ_PASS not set, defaulting to {}", default.password);
                default.password
            }),
            queue_name: env::var("RABBITMQ_PRODUCER_QUEUE").unwrap_or_else(|_| {
                warn!("RABBITMQ_PRODUCER_QUEUE not set, defaulting to analytics");
                default.queue_name
            }),
        }
    }
}

/// Configuration for a WebSocket server.
///
/// This struct holds the host and port configuration for a WebSocket server.
/// It can be created either with default values or from environment variables.
///
/// # Environment Variables
/// - `WS_HOST`: The hostname for the WebSocket server (default: "localhost")
/// - `WS_PORT`: The port number for the WebSocket server (default: 8080)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketConfig {
    /// The hostname or IP address the WebSocket server will bind to
    pub host: String,
    /// The port number the WebSocket server will listen on
    pub port: u16,
}

impl Default for WebSocketConfig {
    /// Creates a default configuration for local development.
    ///
    /// # Returns
    /// A new `WebSocketConfig` instance configured to listen on localhost:8080
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 8080,
        }
    }
}

impl FromEnv for WebSocketConfig {
    /// Creates a new configuration instance from environment variables.
    ///
    /// If any environment variable is not set or contains invalid values,
    /// falls back to the default value and logs a warning message using
    /// the `tracing` crate.
    ///
    /// # Returns
    /// A new `WebSocketConfig` instance with values from environment variables or defaults
    ///
    /// # Notes
    /// The port number from the environment must be parseable as a u16,
    /// otherwise the default port will be used.
    fn from_env() -> Self {
        let default = Self::default();

        Self {
            host: env::var("WS_HOST").unwrap_or_else(|_| {
                warn!("WS_HOST not set, defaulting to {}", default.host);
                default.host
            }),
            port: env::var("WS_PORT")
                .map(|port| port.parse().unwrap_or(default.port))
                .map_err(|e| {
                    warn!(
                        "WS_PORT not set or invalid, defaulting to {}: {}",
                        default.port, e
                    );
                    default.port
                })
                .unwrap(),
        }
    }
}
