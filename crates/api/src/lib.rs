//! HTTP API surface for lazypock.
//!
//! Routes follow the PocketBase contract (see `PLAN.md` section 5) so upstream
//! PocketBase clients and tooling work unmodified.

pub mod error;
pub mod records;
pub mod router;
pub mod state;

pub use state::AppState;
