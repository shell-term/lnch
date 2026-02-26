use tokio::sync::mpsc;

/// Set up signal handlers for graceful shutdown.
/// Sends () on shutdown_tx when SIGTERM or SIGINT is received.
pub async fn setup_signal_handler(shutdown_tx: mpsc::Sender<()>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to set up SIGTERM handler");
        let mut sigint =
            signal(SignalKind::interrupt()).expect("Failed to set up SIGINT handler");
        tokio::select! {
            _ = sigterm.recv() => {},
            _ = sigint.recv() => {},
        }
        let _ = shutdown_tx.send(()).await;
    }

    #[cfg(windows)]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to set up Ctrl+C handler");
        let _ = shutdown_tx.send(()).await;
    }
}
