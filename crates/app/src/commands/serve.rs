use crate::shutdown;
use lazypock_api::{AppState, router};
use lazypock_core::{Config, Error, Result};
use lazypock_db::pool::{Db, DbOptions};

pub async fn run(config: Config) -> Result<()> {
    let addr = config.client_addr();

    let db = Db::connect(&config.database_url, &DbOptions::from_config(&config)).await?;

    let state = AppState::new(config, db);
    let app = router().with_state(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| Error::Internal(format!("failed to bind {addr}: {err}")))?;

    tracing::info!(%addr, "listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown::signal())
    .await
    .map_err(|err| Error::Internal(format!("server error: {err}")))?;

    Ok(())
}
