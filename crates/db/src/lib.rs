//! lazypock database layer.
//!
//! Owns the Postgres connection pool, the versioned migration runner, the
//! collection/field model with its DDL engine, and the in-RAM schema cache. The
//! driver is `tokio-postgres` with `deadpool-postgres` for pooling; there is no
//! ORM — the query layer builds SQL explicitly (see `PLAN.md` section 2).

pub mod collections;
pub mod ddl;
pub mod hooks;
pub mod migrate;
pub mod pool;
pub mod query;
pub mod records;
pub mod reflect;
pub mod rls;
pub mod runner;
pub mod schema;
pub mod types;
pub mod validation;
pub mod watch;

/// Re-exported so downstream crates and integration tests use the exact same
/// driver and pool versions without pinning them again.
pub use deadpool_postgres;
pub use tokio_postgres;
