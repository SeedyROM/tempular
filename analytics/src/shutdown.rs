use tokio::{signal::unix, sync::broadcast};
use tracing::info;

/// Handles system shutdown signals (SIGTERM/SIGINT) for Unix-like systems.
///
/// This function sets up handlers for both SIGTERM and SIGINT signals on macOS and Linux
/// systems. When either signal is received, it broadcasts a shutdown message through
/// the provided channel.
///
/// The function will wait for and handle the first signal that arrives, logging which
/// signal was received before broadcasting the shutdown message.
///
/// # Parameters
/// * `shutdown_tx` - A broadcast channel sender used to notify other parts of the
///   application that a shutdown signal was received
///
/// # Panics
/// * If the SIGTERM or SIGINT signal handlers cannot be set up
/// * If the shutdown signal cannot be sent through the broadcast channel
///
/// # Platform Support
/// This implementation is only available on macOS and Linux systems.
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

/// Handles system shutdown signals (Ctrl+C) for non-Unix systems.
///
/// This function sets up a Ctrl+C signal handler for systems that don't support
/// Unix-style signals (e.g., Windows). When a Ctrl+C signal is received, it
/// broadcasts a shutdown message through the provided channel.
///
/// # Parameters
/// * `shutdown_tx` - A broadcast channel sender used to notify other parts of the
///   application that a shutdown signal was received
///
/// # Panics
/// * If the Ctrl+C signal handler cannot be set up
/// * If the shutdown signal cannot be sent through the broadcast channel
///
/// # Platform Support
/// This implementation is used on all platforms except macOS and Linux.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub async fn handle_shutdown_signals(shutdown_tx: broadcast::Sender<()>) {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to setup Ctrl+C handler");

    shutdown_tx
        .send(())
        .expect("Failed to send shutdown signal");
}
