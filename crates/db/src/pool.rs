//! Postgres connection pool.
//!
//! Pool sizing follows the deployment target (many small stateless nodes), so
//! the pool stays small and the total across nodes must fit the server's
//! `max_connections` (see `PLAN.md` section 9).

use std::str::FromStr;
use std::time::Duration;

use lazypock_core::config::Config;
use lazypock_core::error::{Error, Result};
use tokio_postgres::config::SslMode;
use tokio_postgres::NoTls;

/// How long to wait for a free pooled connection before giving up.
pub const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for a new connection to be established.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long an idle connection may stay in the pool.
pub const DEFAULT_RECYCLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Options used to build a [`Db`] handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbOptions {
    /// Maximum number of pooled connections.
    pub pool_size: u32,
    /// Whether to use Postgres prepared statements.
    ///
    /// This is a *query-layer* switch, not a pool setting: `tokio-postgres`
    /// always prepares statements (named `s{N}`), and `deadpool-postgres` adds a
    /// cache that keeps them alive across checkouts. When this is `false` the
    /// query layer must bypass that cache.
    ///
    /// Note the hard constraint this documents: lazypock requires a **direct**
    /// Postgres connection. PgBouncer in transaction mode (and Neon's pooled
    /// endpoint) cannot keep named statements on a pooled server connection,
    /// and lazypock already needs the direct endpoint for `LISTEN/NOTIFY`.
    /// See `PLAN.md` section 18.1.
    pub prepared_statements: bool,
    /// Extra libpq connection options, e.g. `-c search_path=tenant`.
    pub options: Option<String>,
    /// How long to wait for a free connection.
    pub acquire_timeout: Duration,
}

impl Default for DbOptions {
    fn default() -> Self {
        Self {
            pool_size: 5,
            prepared_statements: true,
            options: None,
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
        }
    }
}

impl DbOptions {
    /// Options derived from the process configuration.
    pub fn from_config(config: &Config) -> Self {
        Self {
            pool_size: config.pool_size,
            prepared_statements: config.pg_prepared_statements,
            options: None,
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
        }
    }
}

/// A pooled handle to a lazypock Postgres database.
#[derive(Clone)]
pub struct Db {
    pool: deadpool_postgres::Pool,
    prepared_statements: bool,
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
    /// Connect to `url` and verify the connection actually works.
    ///
    /// Fails loud rather than deferring the failure to the first request.
    pub async fn connect(url: &str, options: &DbOptions) -> Result<Self> {
        let mut pg_config = tokio_postgres::Config::from_str(url)
            .map_err(|err| Error::config(format!("invalid DATABASE_URL: {err}")))?;
        pg_config.application_name("lazypock");

        // TLS is not implemented yet. Refuse to silently downgrade a URL that
        // demands it, and force plaintext otherwise so behaviour is explicit
        // regardless of libpq's `prefer` default.
        match sslmode(url).as_deref() {
            Some("require") | Some("verify-ca") | Some("verify-full") => {
                return Err(Error::unsupported(format!(
                    "DATABASE_URL requests TLS (sslmode={}); this build has no TLS support yet. \
                     Use sslmode=disable or a TLS-enabled build.",
                    sslmode(url).unwrap_or_default()
                )));
            }
            Some("prefer") => {
                tracing::warn!(
                    "DATABASE_URL asks for sslmode=prefer but this build has no TLS support; \
                     connecting without encryption"
                );
            }
            _ => {}
        }
        pg_config.ssl_mode(SslMode::Disable);

        if let Some(extra) = &options.options {
            pg_config.options(extra);
        }

        let manager = deadpool_postgres::Manager::from_config(
            pg_config,
            NoTls,
            deadpool_postgres::ManagerConfig::default(),
        );

        let pool = deadpool_postgres::Pool::builder(manager)
            .max_size(options.pool_size as usize)
            .wait_timeout(Some(options.acquire_timeout))
            .create_timeout(Some(DEFAULT_CONNECT_TIMEOUT))
            .recycle_timeout(Some(DEFAULT_RECYCLE_TIMEOUT))
            .runtime(deadpool::Runtime::Tokio1)
            .build()
            .map_err(|err| {
                Error::internal(format!("failed to build the connection pool: {err}"))
            })?;

        let db = Self {
            pool,
            prepared_statements: options.prepared_statements,
        };

        // Prove the URL, credentials and network path work right now.
        db.health().await.map_err(|err| {
            Error::config(format!(
                "cannot connect to Postgres at {}: {}",
                lazypock_core::config::redact_database_url(url),
                err.message()
            ))
        })?;

        Ok(db)
    }

    /// Connect using the process configuration.
    pub async fn from_config(config: &Config) -> Result<Self> {
        Self::connect(&config.database_url, &DbOptions::from_config(config)).await
    }

    /// Borrow the underlying pool (for LISTEN, job workers, metrics, ...).
    pub fn pool(&self) -> &deadpool_postgres::Pool {
        &self.pool
    }

    /// Whether prepared statements should be used for user queries.
    ///
    /// Consumed by the query layer; see [`DbOptions::prepared_statements`] for
    /// why lazypock requires a direct database connection either way.
    pub fn prepared_statements(&self) -> bool {
        self.prepared_statements
    }

    /// Check out a connection.
    pub async fn client(&self) -> Result<deadpool_postgres::Client> {
        self.pool.get().await.map_err(|err| {
            // Connection errors can include the host but never the password.
            Error::internal(format!("could not acquire a database connection: {err}"))
        })
    }

    /// Round-trip a trivial query.
    pub async fn health(&self) -> Result<()> {
        let client = self.client().await?;
        client.query_one("select 1", &[]).await.map_err(db_error)?;
        Ok(())
    }
}

/// Extract the `sslmode` query parameter from a Postgres URL.
pub fn sslmode(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key.eq_ignore_ascii_case("sslmode") {
            return Some(value.trim().to_ascii_lowercase());
        }
    }
    None
}

/// Map a driver error to the application error model.
///
/// Prefers the structured database error (SQLSTATE + message) over the generic
/// driver text, and never includes credentials.
pub fn db_error(err: tokio_postgres::Error) -> Error {
    match err.as_db_error() {
        Some(db_err) => Error::internal(format!(
            "database error {}: {}",
            db_err.code().code(),
            db_err.message()
        )),
        None => Error::internal(format!("database error: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sslmode_from_urls() {
        assert_eq!(sslmode("postgres://h/db"), None);
        assert_eq!(
            sslmode("postgres://h/db?sslmode=disable"),
            Some("disable".into())
        );
        assert_eq!(
            sslmode("postgres://h/db?application_name=x&sslmode=REQUIRE"),
            Some("require".into())
        );
        assert_eq!(sslmode("postgres://h/db?sslmode="), Some("".into()));
    }

    #[tokio::test]
    async fn tls_required_urls_fail_loud_without_tls_support() {
        let err = Db::connect(
            "postgres://user:pass@localhost:1/db?sslmode=require",
            &DbOptions::default(),
        )
        .await
        .expect_err("sslmode=require must be rejected");
        assert!(matches!(err, Error::Unsupported(_)), "got {err:?}");
        assert!(err.message().contains("TLS"));
        // The failure must not echo the password.
        assert!(!err.message().contains("pass"));
    }

    #[tokio::test]
    async fn invalid_urls_fail_loud() {
        let err = Db::connect("not a url", &DbOptions::default())
            .await
            .expect_err("garbage url must be rejected");
        assert!(matches!(err, Error::Config(_)), "got {err:?}");
    }

    #[test]
    fn options_follow_the_configuration() {
        let config = Config::from_source(|key| match key {
            "DATABASE_URL" => Some("postgres://u:p@h:5432/db".to_string()),
            "LAZYPOCK_POOL_SIZE" => Some("9".to_string()),
            "LAZYPOCK_PG_PREPARED_STATEMENTS" => Some("off".to_string()),
            _ => None,
        })
        .unwrap();
        let options = DbOptions::from_config(&config);
        assert_eq!(options.pool_size, 9);
        assert!(!options.prepared_statements);
    }
}
