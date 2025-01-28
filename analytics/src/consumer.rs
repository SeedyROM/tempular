use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use futures_lite::stream::StreamExt;
use lapin::{options::*, types::FieldTable, Channel, Connection, ConnectionProperties};
use tracing::{error, info};

use crate::{config::RabbitMQConfig, error::RabbitMQError};

#[async_trait]
pub trait MessageHandler {
    async fn handle_message(&self, topic: &str, payload: &[u8]) -> Result<(), RabbitMQError>;
}

pub struct RabbitMQConsumer {
    channel: Channel,
    queue_name: String,
}

impl RabbitMQConsumer {
    pub async fn new(config: &RabbitMQConfig) -> Result<Self, RabbitMQError> {
        let conn =
            Connection::connect(&config.connection_string(), ConnectionProperties::default())
                .await?;

        let channel = conn.create_channel().await?;

        // Declare exchange
        channel
            .exchange_declare(
                "amq.topic",
                lapin::ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        // Create queue
        let queue = channel
            .queue_declare(
                &config.queue_name,
                QueueDeclareOptions {
                    durable: true,      // Queue will survive broker restart
                    exclusive: false,   // Allow multiple connections
                    auto_delete: false, // Don't delete when consumers disconnect
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;
        info!("Declared queue: {}", queue.name());

        Ok(Self {
            channel,
            queue_name: queue.name().to_string(),
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

        loop {
            // Handle incoming messages and shutdowns
            tokio::select! {
                Some(delivery_result) = consumer.next() => {
                    match delivery_result {
                        Ok(delivery) => {
                            let routing_key = delivery.routing_key.as_str();

                            match handle_with_retry(&handler, routing_key, &delivery.data, MAX_RETRIES).await {
                                Ok(_) => {
                                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                                        error!("Failed to acknowledge message: {}", e);
                                    }
                                }
                                Err(e) => {
                                    if let Err(nack_err) = delivery
                                        .nack(BasicNackOptions {
                                            requeue: true,
                                            ..Default::default()
                                        })
                                        .await
                                    {
                                        error!("Failed to negative acknowledge message: {}", nack_err);
                                    }
                                    error!("Error processing message: {}", e);
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
