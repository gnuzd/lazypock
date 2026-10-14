//! API keys: hashed credentials for non-interactive clients.
//!
//! Stored as a JSON array under `_settings["auth.api_keys"]` — the `auth.` prefix
//! keeps `GET /api/settings` from returning it — with each key kept only as a
//! SHA-256 digest. The raw key is returned once, when it is created.
//!
//! An API key authenticates a **scoped** identity. It is not a superuser and has no
//! record identity, so it carries no superuser bypass and no `@request.auth.*`; the
//! only endpoints that accept it are the ones that explicitly say so (the
//! collection reads codegen needs). Everything else that is superuser-only rejects
//! it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use lazypock_core::error::Error;
use lazypock_core::id::new_id;
use lazypock_db::deadpool_postgres::Client;
use lazypock_db::pool::db_error;

use crate::error::ApiError;

/// The `_settings` key the list lives under.
pub const SETTINGS_KEY: &str = "auth.api_keys";

/// The prefix every generated key carries, which is also how the auth middleware
/// recognises one without a database lookup for the common JWT case.
pub const PREFIX: &str = "lazypock_";

/// Bytes of entropy in a generated key.
const KEY_BYTES: usize = 32;

/// One stored key. The raw key is never part of this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Stable id, used to revoke.
    pub id: String,
    /// SHA-256 digest of the raw key, hex.
    pub hash: String,
    /// When the key was created (RFC 3339, UTC).
    #[serde(default)]
    pub created_at: String,
    /// When it stops working, if it does (RFC 3339, UTC).
    #[serde(default)]
    pub expires_at: Option<String>,
    /// Whether it has been revoked.
    #[serde(default)]
    pub revoked: bool,
}

impl ApiKey {
    /// The shape a client reads: no hash.
    pub fn public(&self) -> Value {
        serde_json::json!({
            "id": self.id,
            "created_at": self.created_at,
            "expires_at": self.expires_at,
            "revoked": self.revoked,
        })
    }
}

/// Every stored key, oldest first.
pub async fn list(client: &Client) -> Result<Vec<ApiKey>, ApiError> {
    load(client).await
}

/// Create a key, returning the raw value to show once and its stored record.
pub async fn create(
    client: &Client,
    expires_in_days: Option<i64>,
) -> Result<(String, ApiKey), ApiError> {
    let raw = new_key();
    let created = now_seconds();

    let key = ApiKey {
        id: new_id(),
        hash: digest(&raw),
        created_at: rfc3339(created),
        expires_at: expires_in_days
            .filter(|days| *days > 0)
            .map(|days| rfc3339(created + days * 86_400)),
        revoked: false,
    };

    let mut keys = load(client).await?;
    keys.push(key.clone());
    store(client, &keys).await?;
    Ok((raw, key))
}

/// Revoke a key. Returns whether one with that id existed.
///
/// A revoked key is kept rather than deleted, so the list can still show that it
/// was revoked and when it was created.
pub async fn revoke(client: &Client, id: &str) -> Result<bool, ApiError> {
    let mut keys = load(client).await?;
    let Some(key) = keys.iter_mut().find(|key| key.id == id) else {
        return Ok(false);
    };
    key.revoked = true;
    store(client, &keys).await?;
    Ok(true)
}

/// Whether a presented value is an active key.
///
/// Only values carrying the prefix reach the database, so a JWT is not looked up.
pub async fn verify(client: &Client, presented: &str) -> Result<bool, ApiError> {
    if !presented.starts_with(PREFIX) {
        return Ok(false);
    }
    let presented = digest(presented);
    let now = rfc3339(now_seconds());

    // The comparison is constant time, so a wrong key cannot be narrowed one byte
    // at a time by timing.
    Ok(load(client).await?.iter().any(|key| {
        !key.revoked
            && key
                .expires_at
                .as_deref()
                .is_none_or(|expires| expires > now.as_str())
            && key.hash.as_bytes().ct_eq(presented.as_bytes()).into()
    }))
}

/// Read the stored list.
///
/// A value that cannot be read as a list is treated as no keys rather than an error
/// on every request; a settings row is not a credential the way a malformed policy
/// would be.
async fn load(client: &Client) -> Result<Vec<ApiKey>, ApiError> {
    let row = client
        .query_opt(
            "select value from _settings where key = $1",
            &[&SETTINGS_KEY.to_string()],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let Some(row) = row else {
        return Ok(Vec::new());
    };
    let value: Value = row.get("value");
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// Write the stored list.
async fn store(client: &Client, keys: &[ApiKey]) -> Result<(), ApiError> {
    let value = serde_json::to_value(keys).map_err(|err| {
        ApiError::new(Error::internal(format!(
            "cannot serialize the API keys: {err}"
        )))
    })?;

    client
        .execute(
            "insert into _settings (key, value, updated) values ($1, $2, now()) \
             on conflict (key) do update set value = excluded.value, updated = now()",
            &[&SETTINGS_KEY.to_string(), &value],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;
    Ok(())
}

/// A fresh raw key: the prefix plus 32 random bytes, hex.
fn new_key() -> String {
    let mut bytes = [0u8; KEY_BYTES];
    getrandom::fill(&mut bytes).expect("OS random number generator unavailable");

    let mut key = String::with_capacity(PREFIX.len() + KEY_BYTES * 2);
    key.push_str(PREFIX);
    for byte in bytes {
        key.push_str(&format!("{byte:02x}"));
    }
    key
}

/// The hex SHA-256 digest of a key.
fn digest(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Now, as Unix seconds.
fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Format Unix seconds as RFC 3339 UTC, fixed width so string comparison orders
/// correctly.
fn rfc3339(seconds: i64) -> String {
    lazypock_db::backup::format_rfc3339(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_are_prefixed_and_unique() {
        let keys: Vec<String> = (0..100).map(|_| new_key()).collect();
        assert!(keys.iter().all(|key| key.starts_with(PREFIX)), "prefix");
        assert!(
            keys.iter()
                .all(|key| key.len() == PREFIX.len() + KEY_BYTES * 2),
            "length"
        );
        let unique: std::collections::HashSet<&String> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len(), "keys must not repeat");
    }

    #[test]
    fn the_digest_is_stable_and_hides_the_key() {
        let key = new_key();
        let hashed = digest(&key);
        assert_eq!(hashed.len(), 64, "sha-256 hex");
        assert_eq!(hashed, digest(&key), "deterministic");
        assert!(!hashed.contains(&key), "the raw key is not in the digest");
    }

    #[test]
    fn the_public_shape_omits_the_hash() {
        let key = ApiKey {
            id: "k1".into(),
            hash: "deadbeef".into(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            expires_at: None,
            revoked: false,
        };
        let public = key.public();
        assert!(public.get("hash").is_none(), "{public}");
        assert_eq!(public["id"], serde_json::json!("k1"));
    }
}
