use futures_lite::stream::StreamExt;
use lapin::{options::*, types::FieldTable, Connection, ConnectionProperties, Result};
use std::env;
use tokio;

#[tokio::main]
async fn main() -> Result<()> {
    // Get configuration from environment variables
    let rabbitmq_host = env::var("RABBITMQ_HOST").unwrap_or_else(|_| "localhost".to_string());
    let rabbitmq_port = env::var("RABBITMQ_PORT").unwrap_or_else(|_| "5672".to_string());
    let rabbitmq_user = env::var("RABBITMQ_USER").unwrap_or_else(|_| "guest".to_string());
    let rabbitmq_pass = env::var("RABBITMQ_PASS").unwrap_or_else(|_| "guest".to_string());

    // Connection URL
    let addr = format!(
        "amqp://{}:{}@{}:{}",
        rabbitmq_user, rabbitmq_pass, rabbitmq_host, rabbitmq_port
    );

    // Create a connection
    let conn = Connection::connect(&addr, ConnectionProperties::default()).await?;
    println!("Connected to RabbitMQ");

    // Create a channel
    let channel = conn.create_channel().await?;
    println!("Created channel");

    // The MQTT plugin creates an exchange named 'amq.topic'
    let exchange_name = "amq.topic";
    channel
        .exchange_declare(
            exchange_name,
            lapin::ExchangeKind::Topic,
            ExchangeDeclareOptions {
                passive: true, // Just check if it exists
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    println!("Successfully verified exchange: {}", exchange_name);

    // Create a queue with a unique name
    let queue = channel
        .queue_declare(
            "", // empty string means generate a unique name
            QueueDeclareOptions {
                exclusive: true,
                auto_delete: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    println!("Created queue: {}", queue.name());

    // Bind the queue to the MQTT topics you want to subscribe to
    // MQTT wildcards: + (single level), # (multiple levels)
    // Example: "sensors.#" subscribes to all sensor topics
    let routing_key = "sensors.#"; // Modify this for your MQTT topic pattern
    channel
        .queue_bind(
            queue.name().as_str(),
            exchange_name,
            routing_key,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;
    println!("Bound queue to topic: {}", routing_key);

    // Start consuming messages
    let mut consumer = channel
        .basic_consume(
            queue.name().as_str(),
            "mqtt_consumer",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    println!("Starting to consume MQTT messages");

    while let Some(delivery) = consumer.next().await {
        if let Ok(delivery) = delivery {
            // Get the routing key (MQTT topic)
            let routing_key = delivery.routing_key.as_str();

            // Convert message payload to string
            if let Ok(message) = String::from_utf8(delivery.data.clone()) {
                println!("Received message from topic {}: {}", routing_key, message);

                // Here you can add logic to handle different MQTT topics
                match routing_key {
                    "sensors.temperature" => {
                        // Handle temperature data
                        println!("Temperature reading: {}", message);
                    }
                    "sensors.humidity" => {
                        // Handle humidity data
                        println!("Humidity reading: {}", message);
                    }
                    _ => {}
                }
            }

            // Acknowledge the message
            delivery.ack(BasicAckOptions::default()).await?;
        }
    }

    Ok(())
}
