use deadpool_postgres::{Manager, ManagerConfig, RecyclingMethod, Runtime};
use std::sync::Arc;
use std::time::Duration;
use tokio_postgres_rustls::MakeRustlsConnect;

use lazypock_core::{Config, Error, Result};

/// How long to wait for a free pooled connection before giving up.
pub const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for a new connection to be established.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long an idle connection may stay in the pool.
pub const DEFAULT_RECYCLE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct DbOptions {
    // <- changed: was private (`struct`) but used in a pub fn
    pub pool_size: u32,
    /// Extra libpq connection options, e.g. `-c search_path=tenant`.
    pub options: Option<String>,
    /// How long to wait for a free connection.
    pub acquire_timeout: Duration,
    /// Direct (non-pooled) URL for `LISTEN`; defaults to the main URL. Set this when
    /// the main URL goes through a transaction-mode pooler.
    pub direct_url: Option<String>,
}

impl Default for DbOptions {
    fn default() -> Self {
        Self {
            pool_size: 5,
            options: None,
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
            direct_url: None,
        }
    }
}

impl DbOptions {
    pub fn from_config(config: &Config) -> Self {
        Self {
            pool_size: config.pool_size,
            options: None,
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
            direct_url: None,
        }
    }
}

/// A pooled handle to a lazypock Postgres database.
#[derive(Clone)]
pub struct Db {
    pool: deadpool_postgres::Pool,
    direct_url: String,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = self.pool.status();
        f.debug_struct("Db")
            .field("size", &status.size)
            .field("available", &status.available)
            .finish()
    }
}

impl Db {
    pub async fn connect(url: &str, options: &DbOptions) -> Result<Self> {
        let mut pg_config: tokio_postgres::Config = url
            .parse()
            .map_err(|err| Error::Config(format!("invalid database URL: {err}")))?;

        pg_config.application_name("lazypock");

        if let Some(extra) = &options.options {
            pg_config.options(extra);
        }

        let manager = Manager::from_config(
            pg_config,
            tls_connector()?,
            ManagerConfig {
                // `Fast` only checks the socket is open. `Clean` would DISCARD ALL,
                // which a transaction pooler does not need and some forbid.
                recycling_method: RecyclingMethod::Fast,
            },
        );

        let pool = deadpool_postgres::Pool::builder(manager)
            .max_size(options.pool_size.max(1) as usize)
            .wait_timeout(Some(options.acquire_timeout))
            .create_timeout(Some(DEFAULT_CONNECT_TIMEOUT))
            .recycle_timeout(Some(DEFAULT_RECYCLE_TIMEOUT))
            .runtime(Runtime::Tokio1)
            .build()
            .map_err(|err| {
                Error::Internal(format!("failed to build the connection pool: {err}"))
            })?;

        let db = Self {
            pool,
            direct_url: options
                .direct_url
                .clone()
                .unwrap_or_else(|| url.to_string()),
        };

        db.health().await?;

        Ok(db)
    }

    /// Connect using the process configuration.
    pub async fn from_config(config: &Config) -> Result<Self> {
        Self::connect(&config.database_url, &DbOptions::from_config(config)).await
    }

    pub async fn health(&self) -> Result<()> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|err| Error::Internal(format!("failed to connect to database")))?;

        Ok(())
    }
}

fn tls_connector() -> Result<MakeRustlsConnect> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // Provider is passed explicitly: no reliance on a process-wide default, so
    // adding another rustls consumer later cannot make this panic.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Config(format!("tls: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(cfg))
}

// impl Db {
//     /// URL for session-scoped features (`LISTEN`); not used for request traffic.
//     pub fn direct_url(&self) -> &str {
//         &self.direct_url
//     }
//
//     /// Check out a connection.
//     pub async fn conn(&self) -> Result<deadpool_postgres::Object> {
//         use crate::error::PgExt;
//         self.pool.get().await.pg()
//     }
//
//     /// Cheap round trip with a hard 2 s ceiling (for readiness probes).
//     pub async fn ping(&self) -> Result<()> {
//         use crate::error::PgExt;
//         let probe = async {
//             let conn = self.conn().await?;
//             conn.simple_query("SELECT 1").await.pg()?;
//             Ok::<(), Error>(())
//         };
//         match tokio::time::timeout(Duration::from_secs(2), probe).await {
//             Ok(r) => r,
//             Err(_) => Err(Error::Database("ping timed out".into())),
//         }
//     }
// }
