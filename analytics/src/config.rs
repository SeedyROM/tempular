use anyhow::Result;
use color_eyre::Report;
use dotenv::dotenv;
use std::env;
use tracing::warn;

use crate::errors::ConfigError;

#[derive(Debug, Clone)]
pub struct RabbitMQConfig {
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub queue_name: String,
}

impl RabbitMQConfig {
    pub fn from_env() -> Result<Self, Report> {
        dotenv().map_err(ConfigError::DotEnvError)?;

        Ok(Self {
            host: env::var("RABBITMQ_HOST").unwrap_or_else(|_| {
                warn!("RABBITMQ_HOST not set, defaulting to localhost");
                "localhost".to_string()
            }),
            port: env::var("RABBITMQ_PORT").unwrap_or_else(|_| {
                warn!("RABBITMQ_PORT not set, defaulting to 5672");
                "5672".to_string()
            }),
            username: env::var("RABBITMQ_USER").unwrap_or_else(|_| {
                warn!("RABBITMQ_USER not set, defaulting to guest");
                "guest".to_string()
            }),
            password: env::var("RABBITMQ_PASS").unwrap_or_else(|_| {
                warn!("RABBITMQ_PASS not set, defaulting to guest");
                "guest".to_string()
            }),
            queue_name: env::var("RABBITMQ_PRODUCER_QUEUE").unwrap_or_else(|_| {
                warn!("RABBITMQ_PRODUCER_QUEUE not set, defaulting to analytics");
                "sensor_data".to_string()
            }),
        })
    }

    pub fn connection_string(&self) -> String {
        format!(
            "amqp://{}:{}@{}:{}",
            self.username, self.password, self.host, self.port
        )
    }
}
