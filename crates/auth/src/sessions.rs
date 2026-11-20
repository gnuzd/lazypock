//! Refresh-token sessions (`_sessions`).
//!
//! - The client holds an opaque random token; only its SHA-256 is stored, so a
//!   leaked table cannot be replayed.
//! - Every use **rotates** it: the old row is revoked and a new one is issued in the
//!   same *family*.
//! - Presenting an already-rotated token means it was copied; the whole family is
//!   revoked, signing the thief and the owner out together.

use crate::pg::{q, q_opt};
use base64::Engine;
use lazypock_core::id::new_id;
use lazypock_core::{Error, Result};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime};
use tokio_postgres::{GenericClient, Row};

pub const REFRESH_TOKEN_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

const COLUMNS: &str = "id, collection_id, user_id, family_id, expires_at, revoked_at";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    /// Empty for a superuser.
    pub collection_id: String,
    pub user_id: String,
    pub family_id: String,
    pub expires_at: SystemTime,
    pub revoked: bool,
}

pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 32 random bytes, base64url (256 bits).
pub fn generate_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
        .map_err(|_| Error::Internal("the system random number generator is unavailable".into()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// Start a new session (a new family). Returns the refresh token to hand out.
pub async fn issue<C: GenericClient>(
    client: &C,
    collection_id: &str,
    user_id: &str,
    user_agent: Option<&str>,
    ip: Option<&str>,
) -> Result<(String, Session)> {
    issue_in_family(client, collection_id, user_id, &new_id(), user_agent, ip).await
}

async fn issue_in_family<C: GenericClient>(
    client: &C,
    collection_id: &str,
    user_id: &str,
    family_id: &str,
    user_agent: Option<&str>,
    ip: Option<&str>,
) -> Result<(String, Session)> {
    let token = generate_token()?;
    let seconds = REFRESH_TOKEN_TTL.as_secs().to_string();
    let rows = q(
        client,
        &format!(
            "insert into _sessions (id, collection_id, user_id, token_hash, family_id, expires_at, user_agent, ip) \
             values ($1::text, nullif($2::text, ''), $3::text, $4::text, $5::text, \
                     now() + make_interval(secs => $6::text::int), nullif($7::text, ''), nullif($8::text, '')) \
             returning {COLUMNS}"
        ),
        &[&new_id(), collection_id, user_id, &hash_token(&token), family_id, &seconds, user_agent.unwrap_or(""), ip.unwrap_or("")],
    )
    .await?;
    let row = rows
        .first()
        .ok_or_else(|| Error::Internal("the session was not stored".into()))?;
    Ok((token, from_row(row)))
}

/// Look a token up without consuming it.
pub async fn peek<C: GenericClient>(client: &C, token: &str) -> Result<Option<Session>> {
    let row = q_opt(
        client,
        &format!("select {COLUMNS} from _sessions where token_hash = $1::text"),
        &[&hash_token(token)],
    )
    .await?;
    Ok(row.as_ref().map(from_row))
}

/// Exchange a refresh token for a new one in the same family.
pub async fn rotate<C: GenericClient>(
    client: &C,
    token: &str,
    user_agent: Option<&str>,
    ip: Option<&str>,
) -> Result<(String, Session)> {
    let hash = hash_token(token);
    // One statement claims the row: of two concurrent uses, exactly one wins.
    let claimed = q_opt(
        client,
        &format!(
            "update _sessions set revoked_at = now() \
             where token_hash = $1::text and revoked_at is null and expires_at > now() \
             returning {COLUMNS}"
        ),
        &[&hash],
    )
    .await?;
    if let Some(row) = claimed {
        let s = from_row(&row);
        return issue_in_family(
            client,
            &s.collection_id,
            &s.user_id,
            &s.family_id,
            user_agent,
            ip,
        )
        .await;
    }

    let Some(row) = q_opt(
        client,
        &format!("select {COLUMNS} from _sessions where token_hash = $1::text"),
        &[&hash],
    )
    .await?
    else {
        return Err(Error::Unauthorized(
            "the refresh token is not recognised".into(),
        ));
    };
    let s = from_row(&row);
    if s.revoked {
        let revoked = revoke_family(client, &s.family_id).await?;
        tracing::warn!(family = %s.family_id, revoked, "a rotated refresh token was replayed; the session family was revoked");
        return Err(Error::Unauthorized(
            "this refresh token has already been used".into(),
        ));
    }
    Err(Error::Unauthorized("the refresh token has expired".into()))
}

pub async fn revoke_family<C: GenericClient>(client: &C, family_id: &str) -> Result<u64> {
    let rows = q(
        client,
        "update _sessions set revoked_at = now() where family_id = $1::text and revoked_at is null returning 1",
        &[family_id],
    )
    .await?;
    Ok(rows.len() as u64)
}

/// Sign an account out everywhere. `collection_id` is empty for a superuser.
pub async fn revoke_for_user<C: GenericClient>(
    client: &C,
    collection_id: &str,
    user_id: &str,
) -> Result<u64> {
    let rows = q(
        client,
        "update _sessions set revoked_at = now() \
         where user_id = $1::text and collection_id is not distinct from nullif($2::text, '') and revoked_at is null \
         returning 1",
        &[user_id, collection_id],
    )
    .await?;
    Ok(rows.len() as u64)
}

/// Housekeeping: delete expired rows. Run it periodically.
pub async fn purge_expired<C: GenericClient>(client: &C) -> Result<u64> {
    Ok(q(
        client,
        "delete from _sessions where expires_at < now() returning 1",
        &[],
    )
    .await?
    .len() as u64)
}

fn from_row(row: &Row) -> Session {
    Session {
        id: row.get(0),
        collection_id: row.get::<_, Option<String>>(1).unwrap_or_default(),
        user_id: row.get(2),
        family_id: row.get(3),
        expires_at: row.get(4),
        revoked: row.get::<_, Option<SystemTime>>(5).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_hashes_are_stable() {
        let (a, b) = (generate_token().unwrap(), generate_token().unwrap());
        assert_ne!(a, b);
        assert_eq!(a.len(), 43, "32 bytes in base64url without padding");
        assert_eq!(
            hash_token("x"),
            "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881"
        );
        assert_ne!(hash_token(&a), a, "only the hash is stored");
    }
}
