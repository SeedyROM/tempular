mod config;
mod consumer;
mod error;
mod logging;

use anyhow::Result;
use color_eyre::Report;
use config::RabbitMQConfig;

use crate::consumer::{RabbitMQConsumer, SensorMessageHandler};

#[tokio::main]
async fn main() -> Result<(), Report> {
    logging::setup()?;

    let config = RabbitMQConfig::from_env()?;

    let consumer = RabbitMQConsumer::new(&config).await?;
    consumer.bind_topic("sensors.#").await?;

    let handler = SensorMessageHandler;
    consumer.start_consuming(handler).await?;

    Ok(())
}

// For testing, create a mock implementation
#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use consumer::MessageHandler;
    use error::RabbitMQError;

    struct MockMessageHandler {
        messages: std::sync::Arc<tokio::sync::Mutex<Vec<(String, Vec<u8>)>>>,
    }

    #[async_trait]
    impl MessageHandler for MockMessageHandler {
        async fn handle_message(&self, topic: &str, payload: &[u8]) -> Result<(), RabbitMQError> {
            let mut messages = self.messages.lock().await;
            messages.push((topic.to_string(), payload.to_vec()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_message_handler() {
        let messages = std::sync::Arc::new(tokio::sync::Mutex::new(vec![]));
        let handler = MockMessageHandler {
            messages: messages.clone(),
        };

        handler
            .handle_message("sensors.temperature", b"25.5")
            .await
            .unwrap();

        let received = messages.lock().await;
        assert_eq!(received[0].0, "sensors.temperature");
        assert_eq!(received[0].1, b"25.5");
    }
}
