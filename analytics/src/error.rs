use thiserror::Error;

#[allow(unused)]
#[derive(Error, Debug)]
pub enum RabbitMQError {
    #[error("Failed to connect to RabbitMQ: {0}")]
    ConnectionError(#[from] lapin::Error),

    #[error("Failed to parse message: {0}")]
    MessageParseError(String),
}
