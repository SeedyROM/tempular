use std::env;
use tracing::warn;

use crate::traits::FromEnv;

#[derive(Debug, Clone)]
pub struct RabbitMQConfig {
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub queue_name: String,
}

impl Default for RabbitMQConfig {
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
    pub fn url(&self) -> String {
        format!(
            "amqp://{}:{}@{}:{}",
            self.username, self.password, self.host, self.port
        )
    }
}

impl FromEnv for RabbitMQConfig {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketConfig {
    pub host: String,
    pub port: u16,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 8080,
        }
    }
}

impl FromEnv for WebSocketConfig {
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
