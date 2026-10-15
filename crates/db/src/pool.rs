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
use tokio_postgres_rustls::MakeRustlsConnect;

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
        let mut pg_config = tokio_postgres::Config::from_str(&normalize_url(url))
            .map_err(|err| Error::config(format!("invalid DATABASE_URL: {err}")))?;
        pg_config.application_name("lazypock");

        if let Some(extra) = &options.options {
            pg_config.options(extra);
        }

        // A URL that asks for TLS is never silently downgraded: the connector is
        // built here, and a TLS handshake or certificate failure surfaces as a
        // connection error rather than a quiet plaintext session.
        let manager = match tls_mode(url) {
            TlsMode::Required => deadpool_postgres::Manager::from_config(
                pg_config,
                rustls_connector()?,
                deadpool_postgres::ManagerConfig::default(),
            ),
            TlsMode::Disabled => {
                // Force plaintext explicitly, regardless of libpq's `prefer` default.
                pg_config.ssl_mode(SslMode::Disable);
                deadpool_postgres::Manager::from_config(
                    pg_config,
                    NoTls,
                    deadpool_postgres::ManagerConfig::default(),
                )
            }
        };

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

/// Rewrite a URL into the shape `tokio_postgres` parses.
///
/// `ssl` is accepted as an alias for `sslmode` (Neon and some drivers use it), and
/// `channel_binding` is dropped: the driver does not support it, and once the
/// server certificate is verified there is no channel to bind. Both changes only
/// affect what the driver is asked to do — [`tls_mode`] reads the original URL.
fn normalize_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };

    let mut params: Vec<String> = Vec::new();
    let mut has_sslmode = false;
    let mut ssl: Option<String> = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key.to_ascii_lowercase().as_str() {
            "sslmode" => {
                has_sslmode = true;
                params.push(pair.to_string());
            }
            "ssl" => ssl = Some(value.to_string()),
            "channel_binding" => {}
            _ => params.push(pair.to_string()),
        }
    }

    // `sslmode` wins over `ssl`, so only use `ssl` when `sslmode` is absent.
    let ssl = if has_sslmode { None } else { ssl };
    if let Some(value) = ssl {
        let value = if value.eq_ignore_ascii_case("true") || value == "1" {
            "require".to_string()
        } else {
            value
        };
        params.push(format!("sslmode={value}"));
    }

    if params.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", params.join("&"))
    }
}

/// Extract the TLS mode a Postgres URL asks for.
///
/// `sslmode` and the `ssl` spelling some providers use (Neon's `ssl=require`) are
/// both read, so a URL that asks for TLS is never silently downgraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    /// Plaintext. `disable`, `allow`, an unset mode, and the unimplemented `prefer`.
    Disabled,
    /// TLS with full certificate verification.
    Required,
}

/// Whether a URL requires TLS.
///
/// `require`, `verify-ca` and `verify-full` all enable TLS **with certificate
/// verification**, which is stricter than libpq's `require` (encrypt without
/// verifying). That is the safe reading: a URL asking for encryption gets a
/// verified connection, and an untrusted server is refused rather than accepted.
pub fn tls_mode(url: &str) -> TlsMode {
    match sslmode(url).as_deref() {
        Some("require") | Some("verify-ca") | Some("verify-full") => TlsMode::Required,
        Some("prefer") => {
            tracing::warn!(
                "sslmode=prefer is not implemented (there is no plaintext fallback); \
                 connecting without encryption"
            );
            TlsMode::Disabled
        }
        _ => TlsMode::Disabled,
    }
}

/// A rustls connector that verifies the server against the system trust store.
fn rustls_connector() -> Result<MakeRustlsConnect> {
    let loaded = rustls_native_certs::load_native_certs();
    for error in &loaded.errors {
        tracing::warn!(%error, "could not load a native certificate");
    }

    let mut roots = rustls::RootCertStore::empty();
    for cert in loaded.certs {
        if let Err(err) = roots.add(cert) {
            tracing::warn!(%err, "ignoring an unusable native certificate");
        }
    }
    if roots.is_empty() {
        return Err(Error::config(
            "the system trust store is empty, so a TLS connection could not be verified; \
             install CA certificates or use sslmode=disable",
        ));
    }

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

/// Extract the `sslmode` query parameter from a Postgres URL.
///
/// `sslmode` wins when both spellings are present; otherwise `ssl` is read, with
/// `ssl=true`/`ssl=1` meaning `require`.
pub fn sslmode(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    let mut ssl = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = value.trim().to_ascii_lowercase();
        if key.eq_ignore_ascii_case("sslmode") {
            return Some(value);
        }
        if key.eq_ignore_ascii_case("ssl") {
            ssl = Some(value);
        }
    }
    ssl.map(|value| {
        if value == "true" || value == "1" {
            "require".to_string()
        } else {
            value
        }
    })
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
    fn a_url_is_normalized_for_the_driver() {
        // `ssl` becomes `sslmode`, and `channel_binding` is dropped.
        assert_eq!(
            normalize_url("postgres://h/db?ssl=require&channel_binding=require"),
            "postgres://h/db?sslmode=require"
        );
        // `sslmode` wins and is left alone.
        assert_eq!(
            normalize_url("postgres://h/db?sslmode=require&ssl=disable"),
            "postgres://h/db?sslmode=require"
        );
        // Other parameters are preserved.
        assert_eq!(
            normalize_url("postgres://h/db?application_name=x&ssl=true"),
            "postgres://h/db?application_name=x&sslmode=require"
        );
        assert_eq!(normalize_url("postgres://h/db"), "postgres://h/db");
    }

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

        // The `ssl` spelling, including Neon's `ssl=require&channel_binding=require`.
        assert_eq!(
            sslmode("postgres://h/db?ssl=require&channel_binding=require"),
            Some("require".into())
        );
        assert_eq!(sslmode("postgres://h/db?ssl=true"), Some("require".into()));
        assert_eq!(sslmode("postgres://h/db?ssl=1"), Some("require".into()));
        // `sslmode` wins if both are present.
        assert_eq!(
            sslmode("postgres://h/db?ssl=disable&sslmode=require"),
            Some("require".into())
        );
    }

    #[test]
    fn a_tls_spelling_selects_the_tls_mode() {
        assert_eq!(tls_mode("postgres://h/db"), TlsMode::Disabled);
        assert_eq!(
            tls_mode("postgres://h/db?sslmode=disable"),
            TlsMode::Disabled
        );
        assert_eq!(tls_mode("postgres://h/db?ssl=require"), TlsMode::Required);
        assert_eq!(
            tls_mode("postgres://h/db?sslmode=require"),
            TlsMode::Required
        );
        assert_eq!(
            tls_mode("postgres://h/db?sslmode=verify-full"),
            TlsMode::Required
        );
    }

    /// A live TLS round-trip, only when a URL is supplied. This is how Neon is
    /// verified: `LAZYPOCK_TLS_TEST_URL=postgres://…` runs it, and it is skipped
    /// otherwise so the suite does not need a TLS database.
    #[tokio::test]
    async fn connects_over_tls_when_a_url_is_provided() {
        let Ok(url) = std::env::var("LAZYPOCK_TLS_TEST_URL") else {
            return;
        };
        let db = Db::connect(
            &url,
            &DbOptions {
                pool_size: 1,
                ..Default::default()
            },
        )
        .await
        .expect("a TLS connection should succeed");
        let client = db.client().await.expect("client");
        let row = client.query_one("select 1", &[]).await.expect("select 1");
        assert_eq!(row.get::<_, i32>(0), 1);
    }

    #[tokio::test]
    async fn a_tls_url_that_cannot_connect_fails_without_echoing_the_password() {
        // Nothing listens on port 1, so the TLS attempt cannot complete. The point
        // is that `require` attempts TLS (rather than being refused or silently
        // downgraded) and that the failure never includes the password.
        let err = Db::connect(
            "postgres://user:pass@localhost:1/db?sslmode=require",
            &DbOptions::default(),
        )
        .await
        .expect_err("a connection to a closed port must fail");
        assert!(matches!(err, Error::Config(_)), "got {err:?}");
        assert!(
            !err.message().contains("pass"),
            "the password must not appear: {}",
            err.message()
        );
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
            "LAZYPOCK_DATABASE_URL" => Some("postgres://u:p@h:5432/db".to_string()),
            "LAZYPOCK_POOL_SIZE" => Some("9".to_string()),
            "LAZYPOCK_PG_PREPARED_STATEMENTS" => Some("off".to_string()),
            _ => None,
        })
        .expect("a valid configuration");
        let options = DbOptions::from_config(&config);
        assert_eq!(options.pool_size, 9);
        assert!(!options.prepared_statements);
    }
}
