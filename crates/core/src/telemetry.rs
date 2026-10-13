//! Tracing / logging setup.

use crate::config::{LogFormat, LogLevel};
use crate::error::{Error, Result};

/// Install the global tracing subscriber.
///
/// `RUST_LOG` takes precedence over the configured level when set.
/// Returns an error if a subscriber is already installed, so tests never call
/// this twice.
pub fn init(level: LogLevel, format: LogFormat) -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level.as_str()));

    let installed = match format {
        LogFormat::Json => tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_target(true)
            .try_init(),
        LogFormat::Text => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .try_init(),
    };

    installed.map_err(|err| Error::config(format!("failed to initialise telemetry: {err}")))
}
