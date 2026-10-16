//! lazypock authentication: password hashing, access tokens and refresh
//! sessions.
//!
//! Kept apart from the HTTP layer so the pieces can be tested directly, and so
//! the record API and the admin API can share exactly the same flows.

pub mod password;
pub mod service;
pub mod sessions;
pub mod superuser;
pub mod token;

pub use service::{AuthOutcome, AuthService};
pub use token::{Claims, SigningKeys, Subject};
