//! lazypock database layer.
//!
//! Owns the Postgres connection pool and the versioned migration runner. The
//! driver is `tokio-postgres` with `deadpool-postgres` for pooling; there is no
//! ORM — the query layer builds SQL explicitly (see `PLAN.md` section 2).

pub mod migrate;
pub mod pool;

/// Re-exported so downstream crates and integration tests use the exact same
/// driver and pool versions without pinning them again.
pub use deadpool_postgres;
pub use tokio_postgres;
