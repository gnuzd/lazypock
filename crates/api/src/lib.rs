//! HTTP API surface for lazypock.
//!
//! Routes follow the PocketBase contract (see `PLAN.md` section 5) so upstream
//! PocketBase clients and tooling work unmodified.

pub mod auth;
pub mod backup;
pub mod batch;
pub mod collections;
pub mod cors;
pub mod error;
pub mod files;
pub mod logs;
pub mod presets;
pub mod realtime;
pub mod records;
pub mod router;
pub mod settings;
pub mod state;
pub mod superusers;

pub use state::{AppState, Services};
