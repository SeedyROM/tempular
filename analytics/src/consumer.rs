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
        let conn =
            Connection::connect(&config.connection_string(), ConnectionProperties::default())
                .await?;
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

    use crate::consumer::MessageHandler;
    use crate::errors::RabbitMQError;

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
