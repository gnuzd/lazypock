use crate::shutdown;
use lazypock_api::{AppState, router};
use lazypock_auth::AuthService;
use lazypock_core::{Config, Error, Result, secret::SecretCipher};
use lazypock_db::{Db, DbOptions, SchemaStore, migrate};
use std::sync::Arc;

pub async fn run(config: Config) -> Result<()> {
    let addr = config.client_addr();

    let db = Db::connect(&config.database_url, &DbOptions::from_config(&config)).await?;
    migrate::run(&db, migrate::SYSTEM).await?;

    let schemas = SchemaStore::new(db.clone());
    schemas.reload().await?;

    // let _watcher = schemas.watch(None);

    let auth = Arc::new(AuthService::new(db.clone(), SecretCipher::from_env()?));
    auth.load().await?;

    let state = AppState::new(config, db, schemas, auth);
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
