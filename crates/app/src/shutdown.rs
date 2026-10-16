//! Graceful shutdown signalling.

/// Resolve when the process receives SIGINT (Ctrl-C) or SIGTERM.
///
/// Used to drain in-flight requests and live realtime connections before exit.
pub async fn signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(err) => {
                tracing::warn!(error = %err, "cannot listen for SIGTERM; only Ctrl-C will stop the server");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = interrupt => tracing::info!("received interrupt"),
        _ = terminate => tracing::info!("received SIGTERM"),
    }
    tracing::info!("shutting down");
}
