//! Runtime configuration, loaded from the environment.
//!
//! Invalid or unrecognised values are rejected loudly ([`Error::Config`])
//! instead of being silently ignored or defaulted: a misconfigured lazypock
//! must fail at boot, not at request time.

use crate::error::{Error, Result};
use crate::net::TrustedProxies;
use std::{env, fmt, net::SocketAddr, path::PathBuf};

/// Default bind address for the API server.
pub const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:4000";
/// Default directory for local data (local file storage, outbox spool, ...).
pub const DEFAULT_DATA_DIR: &str = "data";
/// Default directory for TypeScript hook files.
pub const DEFAULT_HOOKS_DIR: &str = "hooks";
/// Default Postgres pool size per node.
pub const DEFAULT_POOL_SIZE: u32 = 5;
/// Upper bound on the pool size. Postgres dislikes many connections and the
/// deployment target is many small instances, so this is a hard cap.
pub const MAX_POOL_SIZE: u32 = 64;
/// Default maximum upload size (5 MiB).
pub const DEFAULT_UPLOAD_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Deployment environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Env {
    Development,
    Production,
    Test,
}

impl Env {
    /// Parse `LAZYPOCK_ENV`; returns `None` for unrecognised input.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "development" | "dev" => Some(Self::Development),
            "production" | "prod" => Some(Self::Production),
            "test" => Some(Self::Test),
            _ => None,
        }
    }

    /// Canonical lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
            Self::Test => "test",
        }
    }

    /// Whether this is a production deployment.
    pub fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

/// Log verbosity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Parse `LAZYPOCK_LOG_LEVEL`; returns `None` for unrecognised input.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "trace" => Some(Self::Trace),
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    /// Lowercase name accepted by `tracing`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

impl LogFormat {
    /// Parse `LAZYPOCK_LOG_FORMAT`; returns `None` for unrecognised input.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "text" | "pretty" => Some(Self::Text),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    /// Lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }
}

/// Fully resolved runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Deployment environment.
    pub env: Env,
    /// Address the HTTP server binds to.
    pub listen_addr: SocketAddr,
    /// Postgres connection string.
    pub database_url: String,
    /// Directory for local data.
    pub data_dir: PathBuf,
    /// Directory containing TypeScript hook files.
    pub hooks_dir: PathBuf,
    /// Log verbosity.
    pub log_level: LogLevel,
    /// Log output format.
    pub log_format: LogFormat,
    /// Maximum number of pooled Postgres connections per node.
    pub pool_size: u32,
    /// Maximum accepted upload size in bytes.
    pub upload_max_bytes: u64,
    /// Whether Postgres prepared statements are enabled.
    ///
    /// Must be turned off behind PgBouncer in transaction mode (and Neon's
    /// pooled endpoint), which cannot keep named statements across a pooled
    /// server connection. See PLAN.md 18.1.
    pub pg_prepared_statements: bool,
    /// Public base URL of this deployment, when it differs from the request.
    pub public_url: Option<String>,
    /// Peers whose forwarding headers are believed.
    ///
    /// Empty by default, and empty means no header is ever read: a forwarding
    /// header is set by the client when nothing is in front of the server, so
    /// believing one without saying which proxy set it is believing the caller's
    /// own claim about who they are.
    pub trusted_proxies: TrustedProxies,
}

impl Config {
    /// Load configuration from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_source(|key| env::var(key).ok())
    }

    /// Load configuration from an arbitrary key/value source (used by tests).
    pub fn from_source<F>(get: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let env = match optional(&get, "LAZYPOCK_ENV") {
            Some(raw) => Env::parse(&raw).ok_or_else(|| {
                Error::config(format!(
                    "LAZYPOCK_ENV must be one of development|production|test, got {raw:?}"
                ))
            })?,
            None => Env::Development,
        };

        // Only the prefixed name. A bare `DATABASE_URL` is set by plenty of other
        // tooling in the same environment, and honouring it could quietly connect
        // lazypock to somebody else's database.
        let database_url = optional(&get, "LAZYPOCK_DATABASE_URL")
            .ok_or_else(|| Error::config("LAZYPOCK_DATABASE_URL is required"))?;
        validate_database_url(&database_url)?;

        let listen_addr = parse_value(&get, "LAZYPOCK_LISTEN_ADDR", DEFAULT_LISTEN_ADDR)
            .parse::<SocketAddr>()
            .map_err(|e| {
                Error::config(format!("LAZYPOCK_LISTEN_ADDR is not a valid address: {e}"))
            })?;

        let data_dir = PathBuf::from(parse_value(&get, "LAZYPOCK_DATA_DIR", DEFAULT_DATA_DIR));
        let hooks_dir = PathBuf::from(parse_value(&get, "LAZYPOCK_HOOKS_DIR", DEFAULT_HOOKS_DIR));

        let log_level = match optional(&get, "LAZYPOCK_LOG_LEVEL") {
            Some(raw) => LogLevel::parse(&raw).ok_or_else(|| {
                Error::config(format!(
                    "LAZYPOCK_LOG_LEVEL must be one of trace|debug|info|warn|error, got {raw:?}"
                ))
            })?,
            None => LogLevel::Info,
        };

        let log_format = match optional(&get, "LAZYPOCK_LOG_FORMAT") {
            Some(raw) => LogFormat::parse(&raw).ok_or_else(|| {
                Error::config(format!(
                    "LAZYPOCK_LOG_FORMAT must be one of text|json, got {raw:?}"
                ))
            })?,
            None if env.is_production() => LogFormat::Json,
            None => LogFormat::Text,
        };

        let pool_size = match optional(&get, "LAZYPOCK_POOL_SIZE") {
            Some(raw) => {
                let parsed = raw.trim().parse::<u32>().map_err(|_| {
                    Error::config(format!(
                        "LAZYPOCK_POOL_SIZE must be an integer, got {raw:?}"
                    ))
                })?;
                if parsed == 0 || parsed > MAX_POOL_SIZE {
                    return Err(Error::config(format!(
                        "LAZYPOCK_POOL_SIZE must be between 1 and {MAX_POOL_SIZE}, got {parsed}"
                    )));
                }
                parsed
            }
            None => DEFAULT_POOL_SIZE,
        };

        let upload_max_bytes = match optional(&get, "LAZYPOCK_UPLOAD_MAX_BYTES") {
            Some(raw) => {
                let parsed = raw.trim().parse::<u64>().map_err(|_| {
                    Error::config(format!(
                        "LAZYPOCK_UPLOAD_MAX_BYTES must be an integer, got {raw:?}"
                    ))
                })?;
                if parsed == 0 {
                    return Err(Error::config(
                        "LAZYPOCK_UPLOAD_MAX_BYTES must be greater than 0",
                    ));
                }
                parsed
            }
            None => DEFAULT_UPLOAD_MAX_BYTES,
        };

        // A forwarding header is client-controlled unless a proxy set it, so the
        // list of proxies has to be explicit rather than guessed at. A malformed
        // entry is refused at boot: silently trusting nothing, or something, is not
        // a state an operator can notice from the outside.
        let trusted_proxies = match optional(&get, "LAZYPOCK_TRUSTED_PROXIES") {
            Some(raw) => TrustedProxies::parse(&raw)
                .map_err(|err| Error::config(format!("LAZYPOCK_TRUSTED_PROXIES: {err}")))?,
            None => TrustedProxies::default(),
        };

        let public_url = optional(&get, "LAZYPOCK_PUBLIC_URL");
        if let Some(url) = &public_url {
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(Error::config(format!(
                    "LAZYPOCK_PUBLIC_URL must start with http:// or https://, got {url:?}"
                )));
            }
        }

        let pg_prepared_statements = match optional(&get, "LAZYPOCK_PG_PREPARED_STATEMENTS") {
            Some(raw) => parse_bool(&raw).ok_or_else(|| {
                Error::config(format!(
                    "LAZYPOCK_PG_PREPARED_STATEMENTS must be a boolean (true/false), got {raw:?}"
                ))
            })?,
            None => true,
        };

        Ok(Self {
            env,
            listen_addr,
            database_url,
            data_dir,
            hooks_dir,
            log_level,
            log_format,
            pool_size,
            upload_max_bytes,
            pg_prepared_statements,
            public_url,
            trusted_proxies,
        })
    }

    /// The database URL with the password replaced by `***`.
    ///
    /// Safe to log and to print from the CLI.
    pub fn redacted_database_url(&self) -> String {
        redact_database_url(&self.database_url)
    }

    /// Address a client should use to reach this node (unspecified bind
    /// addresses become loopback so `healthcheck` works locally).
    pub fn client_addr(&self) -> SocketAddr {
        if self.listen_addr.ip().is_unspecified() {
            let host = match self.listen_addr {
                SocketAddr::V4(_) => std::net::Ipv4Addr::LOCALHOST.into(),
                SocketAddr::V6(_) => std::net::Ipv6Addr::LOCALHOST.into(),
            };
            SocketAddr::new(host, self.listen_addr.port())
        } else {
            self.listen_addr
        }
    }

    /// Whether a peer's forwarding header may be believed.
    pub fn trusts(&self, peer: std::net::IpAddr) -> bool {
        self.trusted_proxies.contains(peer)
    }

    /// Key/value summary for `lazypock config`, with credentials redacted.
    pub fn summary(&self) -> Vec<(&'static str, String)> {
        vec![
            ("env", self.env.as_str().to_string()),
            ("listen_addr", self.listen_addr.to_string()),
            ("database_url", self.redacted_database_url()),
            ("data_dir", self.data_dir.display().to_string()),
            ("hooks_dir", self.hooks_dir.display().to_string()),
            ("trusted_proxies", self.trusted_proxies.len().to_string()),
            ("log_level", self.log_level.as_str().to_string()),
            ("log_format", self.log_format.as_str().to_string()),
            ("pool_size", self.pool_size.to_string()),
            ("upload_max_bytes", self.upload_max_bytes.to_string()),
            (
                "pg_prepared_statements",
                self.pg_prepared_statements.to_string(),
            ),
            (
                "public_url",
                self.public_url
                    .clone()
                    .unwrap_or_else(|| "(unset)".to_string()),
            ),
        ]
    }
}

fn optional<F>(get: &F, key: &str) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_value<F>(get: &F, key: &str, default: &str) -> String
where
    F: Fn(&str) -> Option<String>,
{
    optional(get, key).unwrap_or_else(|| default.to_string())
}

fn validate_database_url(url: &str) -> Result<()> {
    let rest = url
        .strip_prefix("postgres://")
        .or_else(|| url.strip_prefix("postgresql://"))
        .ok_or_else(|| {
            Error::config(
                "DATABASE_URL must start with postgres:// or postgresql:// \
                 (lazypock only supports Postgres)",
            )
        })?;
    if rest.is_empty() || rest.starts_with('/') {
        return Err(Error::config("DATABASE_URL is missing a host"));
    }
    Ok(())
}

/// Replace the password inside a Postgres URL with `***`.
///
/// Leaves URLs without credentials (or without a password) untouched.
pub fn redact_database_url(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let (scheme, rest) = url.split_at(scheme_end + 3);
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    // The last '@' separates credentials from the host, so passwords that
    // contain '@' are still redacted correctly.
    let Some(at) = authority.rfind('@') else {
        return url.to_string();
    };
    let (credentials, host) = authority.split_at(at + 1);
    let Some(colon) = credentials.find(':') else {
        return url.to_string();
    };
    let user = &credentials[..colon];
    format!("{scheme}{user}:***@{host}{tail}")
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (key, value) in self.summary() {
            writeln!(f, "{key} = {value}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(pairs: &[(&str, &str)]) -> Result<Config> {
        Config::from_source(|key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        })
    }

    fn minimal() -> Vec<(&'static str, &'static str)> {
        vec![(
            "LAZYPOCK_DATABASE_URL",
            "postgres://user:pass@localhost:5432/lazypock",
        )]
    }

    #[test]
    fn minimal_config_gets_documented_defaults() {
        let config = config(&minimal()).expect("valid config");
        assert_eq!(config.env, Env::Development);
        assert_eq!(config.listen_addr, DEFAULT_LISTEN_ADDR.parse().unwrap());
        assert_eq!(config.data_dir, PathBuf::from(DEFAULT_DATA_DIR));
        assert_eq!(config.hooks_dir, PathBuf::from(DEFAULT_HOOKS_DIR));
        assert_eq!(config.log_level, LogLevel::Info);
        assert_eq!(config.log_format, LogFormat::Text);
        assert_eq!(config.pool_size, DEFAULT_POOL_SIZE);
        assert_eq!(config.upload_max_bytes, DEFAULT_UPLOAD_MAX_BYTES);
        assert!(config.pg_prepared_statements);
        assert_eq!(config.public_url, None);
    }

    #[test]
    fn production_defaults_to_json_logs() {
        let mut pairs = minimal();
        pairs.push(("LAZYPOCK_ENV", "production"));
        assert_eq!(config(&pairs).unwrap().log_format, LogFormat::Json);
    }

    #[test]
    fn missing_database_url_is_an_error() {
        let err = config(&[]).expect_err("must fail without a database url");
        assert!(matches!(err, Error::Config(_)));
        assert!(err.message().contains("LAZYPOCK_DATABASE_URL"));
    }

    #[test]
    fn database_url_alias_is_accepted() {
        let config = config(&[("LAZYPOCK_DATABASE_URL", "postgresql://h/db")]).unwrap();
        assert_eq!(config.database_url, "postgresql://h/db");
    }

    #[test]
    fn non_postgres_database_url_is_rejected() {
        let err = config(&[("LAZYPOCK_DATABASE_URL", "mysql://user:pass@localhost/db")])
            .expect_err("must reject non-postgres url");
        assert!(err.message().contains("postgres"));
    }

    #[test]
    fn hostless_database_url_is_rejected() {
        let err =
            config(&[("LAZYPOCK_DATABASE_URL", "postgres:///lazypock")]).expect_err("no host");
        assert!(err.message().contains("host"));
    }

    #[test]
    fn invalid_values_fail_loud_with_the_variable_name() {
        let cases: &[(&str, &str, &str)] = &[
            ("LAZYPOCK_ENV", "staging", "LAZYPOCK_ENV"),
            (
                "LAZYPOCK_LISTEN_ADDR",
                "not-an-address",
                "LAZYPOCK_LISTEN_ADDR",
            ),
            ("LAZYPOCK_LOG_LEVEL", "verbose", "LAZYPOCK_LOG_LEVEL"),
            ("LAZYPOCK_LOG_FORMAT", "xml", "LAZYPOCK_LOG_FORMAT"),
            ("LAZYPOCK_POOL_SIZE", "0", "LAZYPOCK_POOL_SIZE"),
            ("LAZYPOCK_POOL_SIZE", "999", "LAZYPOCK_POOL_SIZE"),
            ("LAZYPOCK_POOL_SIZE", "many", "LAZYPOCK_POOL_SIZE"),
            (
                "LAZYPOCK_UPLOAD_MAX_BYTES",
                "0",
                "LAZYPOCK_UPLOAD_MAX_BYTES",
            ),
            (
                "LAZYPOCK_UPLOAD_MAX_BYTES",
                "big",
                "LAZYPOCK_UPLOAD_MAX_BYTES",
            ),
            (
                "LAZYPOCK_PUBLIC_URL",
                "ftp://example.com",
                "LAZYPOCK_PUBLIC_URL",
            ),
            (
                "LAZYPOCK_PG_PREPARED_STATEMENTS",
                "maybe",
                "LAZYPOCK_PG_PREPARED_STATEMENTS",
            ),
        ];
        for (key, value, expected) in cases {
            let mut pairs = minimal();
            pairs.push((key, value));
            let err = config(&pairs).unwrap_err();
            assert!(
                matches!(err, Error::Config(_)),
                "{key}={value} was accepted"
            );
            assert!(
                err.message().contains(expected),
                "{key}={value} error did not name the variable: {}",
                err.message()
            );
        }
    }

    #[test]
    fn overrides_are_applied() {
        let mut pairs = minimal();
        pairs.extend([
            ("LAZYPOCK_ENV", "test"),
            ("LAZYPOCK_LISTEN_ADDR", "127.0.0.1:9999"),
            ("LAZYPOCK_LOG_LEVEL", "debug"),
            ("LAZYPOCK_LOG_FORMAT", "json"),
            ("LAZYPOCK_POOL_SIZE", "12"),
            ("LAZYPOCK_UPLOAD_MAX_BYTES", "1048576"),
            ("LAZYPOCK_PUBLIC_URL", "https://api.example.com"),
            ("LAZYPOCK_DATA_DIR", "/tmp/lazypock-data"),
            ("LAZYPOCK_PG_PREPARED_STATEMENTS", "off"),
        ]);
        let config = config(&pairs).unwrap();
        assert_eq!(config.env, Env::Test);
        assert_eq!(config.listen_addr, "127.0.0.1:9999".parse().unwrap());
        assert_eq!(config.log_level, LogLevel::Debug);
        assert_eq!(config.log_format, LogFormat::Json);
        assert_eq!(config.pool_size, 12);
        assert_eq!(config.upload_max_bytes, 1_048_576);
        assert_eq!(
            config.public_url.as_deref(),
            Some("https://api.example.com")
        );
        assert_eq!(config.data_dir, PathBuf::from("/tmp/lazypock-data"));
        assert!(!config.pg_prepared_statements);
    }

    #[test]
    fn redacts_passwords_in_database_urls() {
        assert_eq!(
            redact_database_url("postgres://user:secret@host:5432/db?sslmode=require"),
            "postgres://user:***@host:5432/db?sslmode=require"
        );
        assert_eq!(
            redact_database_url("postgres://user:p@ss@host/db"),
            "postgres://user:***@host/db"
        );
        // Nothing to redact: returned unchanged.
        assert_eq!(
            redact_database_url("postgres://user@host/db"),
            "postgres://user@host/db"
        );
        assert_eq!(
            redact_database_url("postgres://host/db"),
            "postgres://host/db"
        );
        assert_eq!(redact_database_url("not-a-url"), "not-a-url");
    }

    #[test]
    fn summary_never_contains_the_password() {
        let config = config(&minimal()).unwrap();
        let rendered = config.to_string();
        assert!(!rendered.contains("pass"), "password leaked: {rendered}");
        assert!(rendered.contains("user:***@localhost"));
    }

    #[test]
    fn unspecified_bind_address_becomes_loopback_for_clients() {
        let cfg = config(&minimal()).unwrap();
        assert_eq!(cfg.client_addr(), "127.0.0.1:4000".parse().unwrap());
        let mut pairs = minimal();
        pairs.push(("LAZYPOCK_LISTEN_ADDR", "10.0.0.5:8080"));
        assert_eq!(
            config(&pairs).unwrap().client_addr(),
            "10.0.0.5:8080".parse().unwrap()
        );
    }
}
