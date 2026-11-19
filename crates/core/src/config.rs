use std::{env, net::SocketAddr};

use crate::error::{Error, Result};
use crate::log::{LogFormat, LogLevel};

#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub log_level: LogLevel,
    pub log_format: LogFormat,
}

impl Config {
    pub fn form_env() -> Result<Self> {
        let listen_addr = env::var("LAZYPOCK_LISTEN_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8000".to_string())
            .parse()
            .map_err(|e| Error::Config(format!("invalid HTTP_ADDR: {e}")))?;

        let log_level = LogLevel::from_env().map_err(Error::Config)?;
        let log_format = LogFormat::from_env().map_err(Error::Config)?;

        Ok(Self {
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
}
