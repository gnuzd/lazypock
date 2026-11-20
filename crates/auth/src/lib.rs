//! Authentication for lazypock.
//!
//! - [`password`]: Argon2id hashing.
//! - [`token`]: Ed25519 JWT access tokens (signed with `ring`, no JWT library).
//! - [`sessions`]: rotating, hashed refresh tokens with replay detection.
//! - [`superuser`]: administrators outside every collection.
//! - [`service`]: [`AuthService`] ties them together: login, refresh, verify.
//!
//! See the crate README for the flows, configuration and wiring in `main`.

#![forbid(unsafe_code)]

mod pg;
pub mod password;
pub mod record;
pub mod service;
pub mod sessions;
pub mod superuser;
pub mod token;

pub use service::{AuthOutcome, AuthService, ClientMeta};
pub use token::{Claims, SigningKeys, Subject, ACCESS_TOKEN_TTL};
