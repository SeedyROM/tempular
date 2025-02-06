use anyhow::Result;
use color_eyre::Report;
use config::AppConfig;
use errors::EnvironmentError;
use tracing::info;

#[allow(unused)]
mod analytics;
mod config;
mod consumer;
mod errors;
mod logging;
#[allow(unused)]
mod messaging;
mod shutdown;
mod websocket;

use crate::consumer::{RabbitMQConsumer, SensorMessageHandler};

#[tokio::main]
async fn main() -> Result<(), Report> {
    // Setup the application
    setup()?;

    // Load configuration
    let config = AppConfig::new()?;

    let consumer = RabbitMQConsumer::new(&config.rabbitmq).await?;
    consumer.bind_topic("sensors.#").await?;

    // Setup shutdown signals
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

    // Start main consumer
    let consumer_handle = tokio::spawn({
        let shutdown_rx = shutdown_tx.subscribe();
        let consumer = consumer.clone();
        async move {
            let handler = SensorMessageHandler;
            consumer.start_consuming(handler, shutdown_rx).await
        }
    });

    // Optionally start DLQ consumer
    let dlq_handle = tokio::spawn({
        let shutdown_rx = shutdown_tx.subscribe();
        async move {
            let handler = SensorMessageHandler;
            consumer.consume_dlq(handler, shutdown_rx).await
        }
    });

    // Start WebSocket server
    let websocket_handle = tokio::spawn({
        let shutdown_rx = shutdown_tx.subscribe();
        websocket::server(config.websocket, shutdown_rx)
    });

    // Setup signal handlers
    tokio::spawn({
        let shutdown_tx = shutdown_tx.clone();
        shutdown::handle_shutdown_signals(shutdown_tx)
    });

    // Wait for both consumers to complete
    let _ = tokio::try_join!(consumer_handle, dlq_handle, websocket_handle)?;

    info!("Application shutdown complete");
    Ok(())
}

fn setup() -> Result<(), Report> {
    dotenvy::dotenv().map_err(|_| EnvironmentError::DotEnvNotFound)?;
    logging::setup()
}
