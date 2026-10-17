//! API keys: hashed credentials for non-interactive clients.
//!
//! Stored in `_api_keys` (migration 0014), each key kept only as a SHA-256 digest; the raw
//! key is returned once, when it is created. It used to be one JSON array under
//! `_settings["auth.api_keys"]`, which made every write a read-modify-write of the whole
//! list — two concurrent creates silently lost one of them — and made every request that
//! carried a key re-read and re-parse the list. A table with an index on the digest fixes
//! both, and lets the expiry comparison happen in SQL against `timestamptz` instead of
//! between two formatted strings.
//!
//! An API key authenticates a **scoped** identity. It is not a superuser and has no
//! record identity, so it carries no superuser bypass and no `@request.auth.*`; the
//! only endpoints that accept it are the ones that explicitly say so (the
//! collection reads codegen needs). Everything else that is superuser-only rejects
//! it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use lazypock_core::id::new_id;
use lazypock_db::deadpool_postgres::Client;
use lazypock_db::pool::db_error;

use crate::error::ApiError;

/// The `_settings` key the list used to live under.
///
/// Nothing reads it any more — migration 0014 moved the keys into `_api_keys` and deleted
/// the row — but the name stays because `GET /api/settings` and the settings export still
/// have to keep ignoring the `auth.` namespace, including on a database that has not
/// migrated yet.
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
    /// SHA-256 digest of the raw key, hex. Never returned to a client.
    #[serde(default)]
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
    pub fn public(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "created_at": self.created_at,
            "expires_at": self.expires_at,
            "revoked": self.revoked,
        })
    }
}

/// The columns every key query selects.
///
/// Timestamps are formatted in SQL so this module needs no date/time dependency, which is
/// the same choice `_files` makes.
const COLUMNS: &str = "id, hash, \
     to_char(created at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as created_at, \
     to_char(expires_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as expires_at, \
     revoked_at is not null as revoked";

/// Every stored key, oldest first.
pub async fn list(client: &Client) -> Result<Vec<ApiKey>, ApiError> {
    let rows = client
        .query(
            &format!("select {COLUMNS} from _api_keys order by created, id"),
            &[],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(rows
        .iter()
        .map(|row| ApiKey {
            id: row.get("id"),
            hash: row.get("hash"),
            created_at: row.get("created_at"),
            expires_at: row.get("expires_at"),
            revoked: row.get("revoked"),
        })
        .collect())
}

/// Create a key, returning the raw value to show once and its stored record.
///
/// An insert rather than a rewrite of the list, so two admins creating a key at the same
/// moment both end up with a working key.
pub async fn create(
    client: &Client,
    expires_in_days: Option<i64>,
) -> Result<(String, ApiKey), ApiError> {
    let raw = new_key();
    let id = new_id();
    let days = expires_in_days
        .filter(|days| *days > 0)
        .map(|days| days.to_string());

    let row = client
        .query_one(
            &format!(
                "insert into _api_keys (id, hash, expires_at) \
                 values ($1::text, $2::text, \
                         case when $3::text is null then null \
                              else now() + make_interval(days => $3::text::int) end) \
                 returning {COLUMNS}"
            ),
            &[&id, &digest(&raw), &days],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let key = ApiKey {
        id: row.get("id"),
        hash: row.get("hash"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
        revoked: row.get("revoked"),
    };
    Ok((raw, key))
}

/// Revoke a key. Returns whether one with that id existed.
///
/// A revoked key is kept rather than deleted, so the list can still show that it was
/// revoked and when it was created. The revocation is one statement, so it cannot be lost
/// by a concurrent create.
pub async fn revoke(client: &Client, id: &str) -> Result<bool, ApiError> {
    let updated = client
        .execute(
            "update _api_keys set revoked_at = coalesce(revoked_at, now()) where id = $1::text",
            &[&id.to_string()],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;
    Ok(updated > 0)
}

/// Whether a presented value is an active key.
///
/// Only values carrying the prefix reach the database, so a JWT is not looked up. The
/// lookup is the digest index, so the cost no longer grows with the number of keys, and
/// expiry is compared by Postgres against `now()` rather than between two strings.
pub async fn verify(client: &Client, presented: &str) -> Result<bool, ApiError> {
    if !presented.starts_with(PREFIX) {
        return Ok(false);
    }

    let found = client
        .query_opt(
            "select 1 from _api_keys \
              where hash = $1::text and revoked_at is null \
                and (expires_at is null or expires_at > now())",
            &[&digest(presented)],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(found.is_some())
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
