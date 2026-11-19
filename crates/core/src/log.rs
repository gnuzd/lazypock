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

    pub fn from_env() -> std::result::Result<Self, String> {
        match std::env::var("LAZYPOCK_LOG_LEVEL") {
            Ok(raw) => Self::parse(&raw).ok_or_else(|| {
                format!(
                    "LAZYPOCK_LOG_LEVEL must be one of: trace | debug | info | warn | error (got: {:?})",
                    raw
                )
            }),
            Err(_) => Ok(Self::Info),
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

    pub fn from_env() -> std::result::Result<Self, String> {
        match std::env::var("LAZYPOCK_LOG_FORMAT") {
            Ok(raw) => Self::parse(&raw).ok_or_else(|| {
                format!(
                    "LAZYPOCK_LOG_LEVEL must be one of: text | json (got: {:?})",
                    raw
                )
            }),
            Err(_) => Ok(Self::Text),
        }
    }
}
