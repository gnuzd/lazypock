//! Storage credentials at rest.
//!
//! The cipher itself lives in [`lazypock_core::secret`], because the access-token
//! signing key needs exactly the same treatment and two implementations of "encrypt this
//! for the database" is one too many. What stays here is why this crate has credentials
//! at all, and the field names a ciphertext is bound to.
//!
//! A superuser can configure storage from the Studio, and that configuration — including
//! the S3 secret — is kept in `_settings`. Anything in `_settings` is also in every
//! backup, every replica and every `pg_dump`, so the secret is stored encrypted, with a
//! key that only the environment holds:
//!
//! ```bash
//! LAZYPOCK_SECRET_KEY=$(openssl rand -hex 32)
//! ```
//!
//! Saving a secret without a key configured is refused rather than stored in the clear.
//! The flip side is stated where it matters: **losing `LAZYPOCK_SECRET_KEY` means the
//! stored secret cannot be read again** — a saved one has to be entered again, and the
//! file backend keeps working meanwhile.

pub use lazypock_core::secret::{SecretCipher as Secrets, KEY_VARIABLE};
