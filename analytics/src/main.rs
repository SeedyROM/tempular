mod config;
mod consumer;
mod error;
mod logging;

use anyhow::Result;
use color_eyre::Report;
use config::RabbitMQConfig;
use tracing::info;

use crate::consumer::{RabbitMQConsumer, SensorMessageHandler};

#[tokio::main]
async fn main() -> Result<(), Report> {
    logging::setup()?;

    let config = RabbitMQConfig::from_env()?;
    let consumer = RabbitMQConsumer::new(&config).await?;
    consumer.bind_topic("sensors.#").await?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
    let (shutdown_tx_clone, shutdown_rx_dlq) = (shutdown_tx.clone(), shutdown_tx.subscribe());

    // Start main consumer
    let consumer_clone = consumer.clone();
    let main_handle = tokio::spawn(async move {
        let handler = SensorMessageHandler;
        consumer_clone.start_consuming(handler, shutdown_rx).await
    });

    // Optionally start DLQ consumer
    let dlq_handle = tokio::spawn(async move {
        let handler = SensorMessageHandler;
        consumer.consume_dlq(handler, shutdown_rx_dlq).await
    });

    // Setup signal handlers
    tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to setup SIGTERM handler");
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .expect("Failed to setup SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => info!("SIGTERM received"),
            _ = sigint.recv() => info!("SIGINT received"),
        }

        shutdown_tx_clone
            .send(())
            .expect("Failed to send shutdown signal");
    });

    // Wait for both consumers to complete
    let _ = tokio::try_join!(main_handle, dlq_handle)?;

    info!("Application shutdown complete");
    Ok(())
}
