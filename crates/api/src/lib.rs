//! HTTP API surface for lazypock.
//!
//! Routes follow the PocketBase contract (see `PLAN.md` section 5) so upstream
//! PocketBase clients and tooling work unmodified.

pub mod api_keys;
pub mod audit;
pub mod auth;
pub mod backup;
pub mod batch;
pub mod client;
pub mod collections;
pub mod cors;
pub mod crons;
pub mod emails;
pub mod error;
pub mod files;
pub mod hooks_host;
pub mod jobs;
pub mod logs;
pub mod mail;
pub mod mail_templates;
pub mod oauth2;
pub mod presets;
pub mod rate_limit;
pub mod realtime;
pub mod records;
pub mod router;
pub mod settings;
pub mod sql;
pub mod state;
pub mod studio;
pub mod superusers;
pub mod views;

pub use state::{AppState, Services};
