//! Errors: this crate has **no error type of its own**; everything is
//! `lazypock_core::Error`, so the API layer maps one type to HTTP.
//!
//! `lazypock_core::Error` carries only a `String`, so driver errors (which can
//! name tables, constraints, hosts) are logged here, once, and replaced by a
//! generic message. The orphan rule forbids `From<tokio_postgres::Error> for
//! Error` in this crate, hence the tiny `.pg()` extension used instead of `?`.

use tokio_postgres::error::SqlState;

use lazypock_core::{Error, Result};

// Constructors with consistent prefixes/status codes (400 unless noted).
pub(crate) fn invalid_ident(m: impl std::fmt::Display) -> Error {
    Error::Validation(format!("invalid identifier: {m}"))
}
pub(crate) fn invalid_schema(m: impl std::fmt::Display) -> Error {
    Error::Validation(format!("invalid schema: {m}"))
}
pub(crate) fn invalid_filter(m: impl std::fmt::Display) -> Error {
    Error::BadRequest(format!("invalid filter: {m}"))
}
pub(crate) fn invalid_sort(m: impl std::fmt::Display) -> Error {
    Error::BadRequest(format!("invalid sort: {m}"))
}
pub(crate) fn not_found(what: impl std::fmt::Display) -> Error {
    Error::NotFound(what.to_string())
}
pub(crate) fn forbidden() -> Error {
    Error::Forbidden("access denied".into())
}

/// Classify a driver error. Expected conditions become 4xx; the rest is logged
/// and surfaces as a generic `Error::Database`.
pub fn pg_error(e: tokio_postgres::Error) -> Error {
    if let Some(state) = e.code() {
        if *state == SqlState::UNIQUE_VIOLATION {
            return Error::Conflict("a record with this value already exists".into());
        }
        if *state == SqlState::FOREIGN_KEY_VIOLATION
            || *state == SqlState::DUPLICATE_TABLE
            || *state == SqlState::DUPLICATE_COLUMN
            || *state == SqlState::DEPENDENT_OBJECTS_STILL_EXIST
        {
            return Error::Conflict("the change conflicts with existing data or schema".into());
        }
        if *state == SqlState::UNDEFINED_TABLE || *state == SqlState::UNDEFINED_COLUMN {
            return Error::NotFound("table or column".into());
        }
        if state.code().starts_with("22") {
            // Data exceptions: a value that does not fit its column (bad date, number out of range, ...).
            return Error::Validation("a value has an invalid format".into());
        }
        if *state == SqlState::NOT_NULL_VIOLATION || *state == SqlState::CHECK_VIOLATION {
            return Error::Validation("a required value is missing or not allowed".into());
        }
        if *state == SqlState::INSUFFICIENT_PRIVILEGE {
            // Also what a violated row-level-security policy reports on writes.
            return forbidden();
        }
    }
    tracing::error!(error = ?e, "postgres error");
    Error::Database("query failed".into())
}

fn pool_error(e: deadpool_postgres::PoolError) -> Error {
    tracing::error!("connection pool error: {e}");
    Error::Database("connection pool unavailable".into())
}

/// `.pg()?` turns driver/pool errors into `lazypock_core::Error`.
/// On a `Result<_, Error>` it is a no-op, so `.await.pg()?` can be used uniformly.
pub trait PgExt<T> {
    fn pg(self) -> Result<T>;
}
impl<T> PgExt<T> for std::result::Result<T, tokio_postgres::Error> {
    fn pg(self) -> Result<T> {
        self.map_err(pg_error)
    }
}
impl<T> PgExt<T> for std::result::Result<T, deadpool_postgres::PoolError> {
    fn pg(self) -> Result<T> {
        self.map_err(pool_error)
    }
}
impl<T> PgExt<T> for std::result::Result<T, Error> {
    fn pg(self) -> Result<T> {
        self
    }
}
