use tokio::{signal::unix, sync::broadcast};
use tracing::info;

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub async fn handle_shutdown_signals(shutdown_tx: broadcast::Sender<()>) {
    let mut sigterm =
        unix::signal(unix::SignalKind::terminate()).expect("Failed to setup SIGTERM handler");
    let mut sigint =
        unix::signal(unix::SignalKind::interrupt()).expect("Failed to setup SIGINT handler");

    tokio::select! {
        _ = sigterm.recv() => info!("SIGTERM received"),
        _ = sigint.recv() => info!("SIGINT received"),
    }

    shutdown_tx
        .send(())
        .expect("Failed to send shutdown signal");
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub async fn handle_shutdown_signals(shutdown_tx: broadcast::Sender<()>) {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to setup Ctrl+C handler");

    shutdown_tx
        .send(())
        .expect("Failed to send shutdown signal");
}
