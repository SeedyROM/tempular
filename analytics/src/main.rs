use anyhow::Result;
use color_eyre::Report;
use config::RabbitMQConfig;
use errors::EnvironmentError;
use tracing::info;
use traits::FromEnv;

mod config;
mod consumer;
mod errors;
mod logging;
mod messaging;
mod shutdown;
mod traits;
mod websocket;

use crate::consumer::{RabbitMQConsumer, SensorMessageHandler};

#[tokio::main]
async fn main() -> Result<(), Report> {
    // Setup the application
    setup()?;

    let config = RabbitMQConfig::from_env();
    let consumer = RabbitMQConsumer::new(&config).await?;
    consumer.bind_topic("sensors.#").await?;

    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

    // Start main consumer
    let main_handle = tokio::spawn({
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
        websocket::server(shutdown_rx)
    });

    // Setup signal handlers
    tokio::spawn({
        let shutdown_tx = shutdown_tx.clone();
        shutdown::handle_shutdown_signals(shutdown_tx)
    });

    // Wait for both consumers to complete
    let _ = tokio::try_join!(main_handle, dlq_handle, websocket_handle)?;

    info!("Application shutdown complete");
    Ok(())
}

fn setup() -> Result<(), Report> {
    dotenv::dotenv().map_err(|_| EnvironmentError::DotEnvNotFound)?;
    logging::setup()
}
