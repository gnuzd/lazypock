//! Application error model.
//!
//! Errors are deliberately coarse and carry a human-readable message. The HTTP
//! layer maps them onto the PocketBase error envelope:
//! `{"status": 400, "message": "...", "data": {}}`.

use std::fmt;

/// Result alias used across the workspace.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Application-level error.
///
/// Unsupported or invalid configuration is reported as [`Error::Config`] rather
/// than being silently ignored: lazypock fails loud on bad input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Invalid or unsupported configuration.
    Config(String),
    /// A recognised feature or setting that is not implemented or enabled.
    Unsupported(String),
    /// The caller supplied a malformed value.
    Invalid(String),
    /// The requested resource does not exist.
    NotFound(String),
    /// The request conflicts with existing state (uniqueness, version, ...).
    Conflict(String),
    /// Authentication is required or the credentials are wrong.
    Unauthorized(String),
    /// Authenticated but not permitted.
    Forbidden(String),
    /// Unexpected internal failure.
    Internal(String),
}

impl Error {
    /// Build a [`Error::Config`].
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// Build a [`Error::Unsupported`].
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }

    /// Build a [`Error::Invalid`].
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    /// Build a [`Error::NotFound`].
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// Build a [`Error::Conflict`].
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    /// Build a [`Error::Unauthorized`].
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    /// Build a [`Error::Forbidden`].
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    /// Build a [`Error::Internal`].
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Return the same error with its message prefixed.
    ///
    /// The variant — and so the status code — is preserved, which is what lets
    /// the batch endpoint say *which* sub-request failed without changing how
    /// the failure is reported.
    pub fn context(self, context: impl fmt::Display) -> Self {
        let prefix = context.to_string();
        match self {
            Self::Config(message) => Self::Config(format!("{prefix}: {message}")),
            Self::Unsupported(message) => Self::Unsupported(format!("{prefix}: {message}")),
            Self::Invalid(message) => Self::Invalid(format!("{prefix}: {message}")),
            Self::NotFound(message) => Self::NotFound(format!("{prefix}: {message}")),
            Self::Conflict(message) => Self::Conflict(format!("{prefix}: {message}")),
            Self::Unauthorized(message) => Self::Unauthorized(format!("{prefix}: {message}")),
            Self::Forbidden(message) => Self::Forbidden(format!("{prefix}: {message}")),
            Self::Internal(message) => Self::Internal(format!("{prefix}: {message}")),
        }
    }

    /// HTTP status code this error maps to.
    pub fn status(&self) -> u16 {
        match self {
            Self::Config(_) | Self::Internal(_) => 500,
            Self::Unsupported(_) => 501,
            Self::Invalid(_) => 400,
            Self::NotFound(_) => 404,
            Self::Conflict(_) => 409,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
        }
    }

    /// Stable machine-readable code, useful in logs and tests.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Config(_) => "config_error",
            Self::Unsupported(_) => "unsupported",
            Self::Invalid(_) => "invalid_request",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::Internal(_) => "internal_error",
        }
    }

    /// Borrow the message.
    pub fn message(&self) -> &str {
        match self {
            Self::Config(m)
            | Self::Unsupported(m)
            | Self::Invalid(m)
            | Self::NotFound(m)
            | Self::Conflict(m)
            | Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::Internal(m) => m,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_match_pocketbase_conventions() {
        assert_eq!(Error::invalid("x").status(), 400);
        assert_eq!(Error::unauthorized("x").status(), 401);
        assert_eq!(Error::forbidden("x").status(), 403);
        assert_eq!(Error::not_found("x").status(), 404);
        assert_eq!(Error::conflict("x").status(), 409);
        assert_eq!(Error::config("x").status(), 500);
        assert_eq!(Error::internal("x").status(), 500);
        assert_eq!(Error::unsupported("x").status(), 501);
    }

    #[test]
    fn display_includes_code_and_message() {
        assert_eq!(
            Error::invalid("bad field").to_string(),
            "invalid_request: bad field"
        );
    }

    #[test]
    fn context_prefixes_the_message_and_keeps_the_variant() {
        let error = Error::not_found("no record").context("batch request 2");
        assert_eq!(error.status(), 404);
        assert_eq!(error.code(), "not_found");
        assert_eq!(error.message(), "batch request 2: no record");
    }
}
