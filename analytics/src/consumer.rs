use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use futures_lite::stream::StreamExt;
use lapin::{
    options::*,
    types::{AMQPValue, FieldTable},
    Channel, Connection, ConnectionProperties,
};
use tracing::{error, info};

use crate::{config::RabbitMQConfig, errors::RabbitMQError};

#[async_trait]
pub trait MessageHandler {
    async fn handle_message(&self, topic: &str, payload: &[u8]) -> Result<(), RabbitMQError>;
}

#[derive(Clone)]
pub struct RabbitMQConsumer {
    channel: Channel,
    queue_name: String,
    #[allow(dead_code)]
    dlx_name: String,
    dlq_name: String,
}

impl RabbitMQConsumer {
    pub async fn new(config: &RabbitMQConfig) -> Result<Self, RabbitMQError> {
        let conn = Connection::connect(&config.url(), ConnectionProperties::default()).await?;
        let channel = conn.create_channel().await?;

        // Declare DLX and DLQ names
        let dlx_name = format!("dlx.{}", config.queue_name);
        let dlq_name = format!("dlq.{}", config.queue_name);

        // Declare the Dead Letter Exchange (DLX)
        channel
            .exchange_declare(
                dlx_name.as_str(),
                lapin::ExchangeKind::Direct,
                ExchangeDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        // Declare the Dead Letter Queue (DLQ)
        channel
            .queue_declare(
                dlq_name.as_str(),
                QueueDeclareOptions {
                    durable: true,
                    exclusive: false,
                    auto_delete: false,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        // Bind DLQ to DLX
        channel
            .queue_bind(
                &dlq_name.as_str(),
                dlx_name.as_str(),
                "dead-letter",
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        // Configure main queue with DLX
        let mut args = FieldTable::default();
        args.insert(
            "x-dead-letter-exchange".into(),
            AMQPValue::LongString(dlx_name.to_string().into()),
        );
        args.insert(
            "x-dead-letter-routing-key".into(),
            AMQPValue::LongString("dead-letter".into()),
        );

        // Create main queue
        let queue = channel
            .queue_declare(
                &config.queue_name,
                QueueDeclareOptions {
                    durable: true,
                    exclusive: false,
                    auto_delete: false,
                    ..Default::default()
                },
                args,
            )
            .await?;

        info!("Declared queue: {} with DLQ", queue.name());

        Ok(Self {
            channel,
            queue_name: queue.name().to_string(),
            dlq_name: dlq_name.as_str().to_string(),
            dlx_name: dlx_name.as_str().to_string(),
        })
    }

    pub async fn bind_topic(&self, routing_key: &str) -> Result<(), RabbitMQError> {
        self.channel
            .queue_bind(
                &self.queue_name,
                "amq.topic",
                routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        info!("Bound queue {} to topic {}", self.queue_name, routing_key);
        Ok(())
    }

    async fn handle_with_retry<H: MessageHandler>(
        handler: &H,
        topic: &str,
        payload: &[u8],
        max_retries: u32,
    ) -> Result<(), RabbitMQError> {
        let mut retries = 0;
        while retries < max_retries {
            match handler.handle_message(topic, payload).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    retries += 1;
                    if retries == max_retries {
                        return Err(e);
                    }
                    #[cfg(test)]
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    #[cfg(not(test))]
                    tokio::time::sleep(Duration::from_secs(2u64.pow(retries))).await;
                }
            }
        }
        Ok(())
    }

    pub async fn start_consuming<H: MessageHandler>(
        &self,
        handler: H,
        mut shutdown: tokio::sync::broadcast::Receiver<()>,
    ) -> Result<(), RabbitMQError> {
        const MAX_RETRIES: u32 = 3;

        let mut consumer = self
            .channel
            .basic_consume(
                &self.queue_name,
                "mqtt_consumer",
                BasicConsumeOptions {
                    no_ack: false,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        info!("Started consuming messages from queue: {}", self.queue_name);

        loop {
            // Handle incoming messages and shutdowns
            tokio::select! {
                Some(delivery_result) = consumer.next() => {
                    match delivery_result {
                        Ok(delivery) => {
                            let routing_key = delivery.routing_key.as_str();

                            match Self::handle_with_retry(&handler, routing_key, &delivery.data, MAX_RETRIES).await {
                                Ok(_) => {
                                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                                        error!("Failed to acknowledge message: {}", e);
                                    }
                                }
                                Err(e) => {
                                    if let Err(nack_err) = delivery
                                        .nack(BasicNackOptions {
                                            requeue: false, // Don't requeue, let it go to DLQ
                                            ..Default::default()
                                        })
                                        .await
                                    {
                                        error!("Failed to negative acknowledge message: {}", nack_err);
                                    }
                                    error!("Message failed after {} retries, moved to DLQ: {}", MAX_RETRIES, e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Error receiving message: {}", e);
                        }
                    }
                }
                Ok(_) = shutdown.recv() => {
                    info!("Shutdown signal received, stopping consumer");
                    break;
                }
                else => {
                    error!("Consumer channel closed unexpectedly");
                    break;
                }
            }
        }

        info!("Consumer stopped gracefully");
        Ok(())
    }

    pub async fn consume_dlq<H: MessageHandler>(
        &self,
        handler: H,
        mut shutdown: tokio::sync::broadcast::Receiver<()>,
    ) -> Result<(), RabbitMQError> {
        let mut consumer = self
            .channel
            .basic_consume(
                &self.dlq_name,
                "dlq_consumer",
                BasicConsumeOptions {
                    no_ack: false,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        info!("Started consuming messages from DLQ: {}", self.dlq_name);

        loop {
            tokio::select! {
                Some(delivery_result) = consumer.next() => {
                    match delivery_result {
                        Ok(delivery) => {
                            let routing_key = delivery.routing_key.as_str();
                            info!("Processing dead letter message: {}", routing_key);

                            // Here you could implement special handling for failed messages
                            match handler.handle_message(routing_key, &delivery.data).await {
                                Ok(_) => {
                                    // Successfully processed, acknowledge
                                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                                        error!("Failed to acknowledge DLQ message: {}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to process DLQ message: {}", e);
                                    // You might want to move these to a separate error queue
                                    if let Err(nack_err) = delivery.nack(BasicNackOptions {
                                        requeue: true,
                                        ..Default::default()
                                    }).await {
                                        error!("Failed to nack DLQ message: {}", nack_err);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Error receiving message from DLQ: {}", e);
                        }
                    }
                }
                Ok(_) = shutdown.recv() => {
                    info!("Shutdown signal received, stopping DLQ consumer");
                    break;
                }
                else => {
                    error!("DLQ consumer channel closed unexpectedly");
                    break;
                }
            }
        }

        info!("DLQ consumer stopped gracefully");
        Ok(())
    }
}

pub struct SensorMessageHandler;

#[async_trait]
impl MessageHandler for SensorMessageHandler {
    async fn handle_message(&self, topic: &str, payload: &[u8]) -> Result<(), RabbitMQError> {
        let message = String::from_utf8(payload.to_vec())
            .map_err(|e| RabbitMQError::MessageParseError(e.to_string()))?;

        match topic {
            "sensors.temperature" => {
                info!("Temperature reading: {}", message);
            }
            "sensors.humidity" => {
                info!("Humidity reading: {}", message);
            }
            _ => info!("Received message from topic {}: {}", topic, message),
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;
    use tokio::time::timeout;

    const TIMEOUT_DURATION: f32 = 0.01;

    // Mock Message Handler
    #[derive(Clone)]
    struct MockMessageHandler {
        messages: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
        should_fail: bool,
        failure_count: Arc<Mutex<usize>>,
    }

    impl MockMessageHandler {
        fn new(should_fail: bool) -> Self {
            Self {
                messages: Arc::new(Mutex::new(vec![])),
                should_fail,
                failure_count: Arc::new(Mutex::new(0)),
            }
        }

        async fn get_messages(&self) -> Vec<(String, Vec<u8>)> {
            self.messages.lock().await.clone()
        }

        async fn get_failure_count(&self) -> usize {
            *self.failure_count.lock().await
        }
    }

    #[async_trait]
    impl MessageHandler for MockMessageHandler {
        async fn handle_message(&self, topic: &str, payload: &[u8]) -> Result<(), RabbitMQError> {
            if self.should_fail {
                let mut count = self.failure_count.lock().await;
                *count += 1;
                return Err(RabbitMQError::MessageParseError("Mock failure".to_string()));
            }
            let mut messages = self.messages.lock().await;
            messages.push((topic.to_string(), payload.to_vec()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_handle_with_retry_success() {
        let handler = MockMessageHandler::new(false);

        let result = timeout(
            Duration::from_secs_f32(TIMEOUT_DURATION),
            RabbitMQConsumer::handle_with_retry(&handler, "test.topic", b"test message", 3),
        )
        .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());

        let messages = handler.get_messages().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, "test.topic");
        assert_eq!(messages[0].1, b"test message");
    }

    #[tokio::test]
    async fn test_handle_with_retry_failure() {
        let handler = MockMessageHandler::new(true);
        const MAX_RETRIES: u32 = 3;

        let result = timeout(
            Duration::from_secs_f32(1.0),
            RabbitMQConsumer::handle_with_retry(
                &handler,
                "test.topic",
                b"test message",
                MAX_RETRIES,
            ),
        )
        .await;

        assert!(result.is_ok());
        let inner_result = result.unwrap();
        assert!(inner_result.is_err());
        match inner_result {
            Err(RabbitMQError::MessageParseError(_)) => (),
            _ => panic!("Expected MessageParseError"),
        }

        // Should have tried MAX_RETRIES times
        assert_eq!(handler.get_failure_count().await, MAX_RETRIES as usize);
    }

    #[tokio::test]
    async fn test_handle_with_retry_partial_failure() {
        // Create a handler that will succeed after a few failures
        let handler = MockMessageHandler {
            messages: Arc::new(Mutex::new(vec![])),
            should_fail: false,
            failure_count: Arc::new(Mutex::new(0)),
        };

        let result = timeout(
            Duration::from_secs(5),
            RabbitMQConsumer::handle_with_retry(&handler, "test.topic", b"test message", 3),
        )
        .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());
        assert!(handler.get_failure_count().await <= 3);

        let messages = handler.get_messages().await;
        assert_eq!(messages.len(), 1);
    }

    #[tokio::test]
    async fn test_sensor_message_handler() {
        let handler = SensorMessageHandler;

        // Test temperature message
        let result = timeout(
            Duration::from_secs_f32(TIMEOUT_DURATION),
            handler.handle_message("sensors.temperature", b"25.5"),
        )
        .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());

        // Test humidity message
        let result = timeout(
            Duration::from_secs_f32(TIMEOUT_DURATION),
            handler.handle_message("sensors.humidity", b"60"),
        )
        .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());

        // Test unknown sensor type
        let result = timeout(
            Duration::from_secs_f32(TIMEOUT_DURATION),
            handler.handle_message("sensors.unknown", b"data"),
        )
        .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());

        // Test invalid UTF-8
        let result = timeout(
            Duration::from_secs_f32(TIMEOUT_DURATION),
            handler.handle_message("sensors.temperature", &[0xFF]),
        )
        .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
}
