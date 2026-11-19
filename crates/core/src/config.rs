use std::{env, net::SocketAddr};

use crate::error::{Error, Result};
use crate::log::{LogFormat, LogLevel};

pub const MAX_POOL_SIZE: u32 = 64;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub pool_size: u32,

    pub listen_addr: SocketAddr,
    pub log_level: LogLevel,
    pub log_format: LogFormat,
}

impl Config {
    pub fn form_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        let listen_addr = env::var("LAZYPOCK_LISTEN_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8000".to_string())
            .parse()
            .map_err(|e| Error::Config(format!("invalid HTTP_ADDR: {e}")))?;

        let database_url = env::var("LAZYPOCK_DATABASE_URL")
            .map_err(|_| Error::Config(format!("LAZYPOCK_DATABASE_URL is required")))?;

        let pool_size = env::var("LAZYPOCK_POOL_SIZE")
            .unwrap_or_else(|_| "5".to_string())
            .parse()
            .map_err(|e| {
                Error::Config(format!(
                    "LAZYPOCK_POOL_SIZE must be between 1 and {MAX_POOL_SIZE}, got {e}"
                ))
            })?;

        let log_level = LogLevel::from_env().map_err(Error::Config)?;
        let log_format = LogFormat::from_env().map_err(Error::Config)?;

        Ok(Self {
            database_url,
            pool_size,
            listen_addr,
            log_level,
            log_format,
        })
    }

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

    pub fn redacted_database_url(&self) -> String {
        redact_database_url(&self.database_url)
    }
}

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
