use anyhow::Result;
use async_trait::async_trait;
use futures_lite::stream::StreamExt;
use lapin::{options::*, types::FieldTable, Channel, Connection, ConnectionProperties};
use tracing::info;

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
    ) -> Result<(), RabbitMQError> {
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

        while let Some(delivery) = consumer.next().await {
            if let Ok(delivery) = delivery {
                let routing_key = delivery.routing_key.as_str();

                match handler.handle_message(routing_key, &delivery.data).await {
                    Ok(_) => {
                        // Only acknowledge if processing succeeded
                        delivery.ack(BasicAckOptions::default()).await?;
                    }
                    Err(e) => {
                        // Negative acknowledge on error, message will be requeued
                        delivery
                            .nack(BasicNackOptions {
                                requeue: true,
                                ..Default::default()
                            })
                            .await?;
                        eprintln!("Error processing message: {}", e);
                    }
                }
            }
        }

        Ok(())
    }
}

// Example message handler implementation
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
