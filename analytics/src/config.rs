use config::{Config, ConfigError, Environment};
use serde::Deserialize;
use tracing::debug;

#[derive(Debug, Deserialize, Clone)]
pub struct RabbitMQConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: String,
    #[serde(default = "default_username")]
    pub username: String,
    #[serde(default = "default_password")]
    pub password: String,
    #[serde(default = "default_queue_name")]
    pub queue_name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WebSocketConfig {
    #[serde(default = "default_ws_host")]
    pub host: String,
    #[serde(default = "default_ws_port")]
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub rabbitmq: RabbitMQConfig,
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
    pub fn url(&self) -> String {
        format!(
            "amqp://{}:{}@{}:{}",
            self.username, self.password, self.host, self.port
        )
    }
}

impl AppConfig {
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
