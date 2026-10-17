//! Refresh sessions.
//!
//! Only a **hash** of the refresh token is stored, so a database leak does not
//! hand out usable tokens. Rotation replaces the row, and presenting a token
//! that has already been rotated is treated as theft: the whole family is
//! revoked so neither the thief nor the victim keeps a usable session.

use std::time::{Duration, SystemTime};

use base64::Engine;
use lazypock_core::error::{Error, Result};
use lazypock_db::pool::db_error;
use lazypock_db::runner::QueryRunner;
use lazypock_db::tokio_postgres::Row;
use sha2::{Digest, Sha256};

/// How long a refresh token stays valid.
pub const REFRESH_TOKEN_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// A stored session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Session id.
    pub id: String,
    /// Collection id, empty for a superuser.
    pub collection_id: String,
    /// The account's record or superuser id.
    pub user_id: String,
    /// Rotations of one login share a family.
    pub family_id: String,
    /// When the refresh token stops being accepted.
    pub expires_at: SystemTime,
    /// Whether this token has already been rotated or revoked.
    pub revoked: bool,
}

/// Hash a refresh token for storage.
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Generate a new opaque token (`ring` CSPRNG, 256 bits).
pub fn generate_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
        .map_err(|_| Error::internal("the system random number generator is unavailable"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// Issue a refresh token in a brand new family, as a login does.
pub async fn issue(
    runner: &dyn QueryRunner,
    collection_id: &str,
    user_id: &str,
    user_agent: Option<&str>,
    ip: Option<&str>,
) -> Result<(String, Session)> {
    let family_id = lazypock_core::id::new_id();
    issue_in_family(runner, collection_id, user_id, &family_id, user_agent, ip).await
}

async fn issue_in_family(
    runner: &dyn QueryRunner,
    collection_id: &str,
    user_id: &str,
    family_id: &str,
    user_agent: Option<&str>,
    ip: Option<&str>,
) -> Result<(String, Session)> {
    let token = generate_token()?;
    let token_hash = hash_token(&token);
    let id = lazypock_core::id::new_id();
    let seconds = REFRESH_TOKEN_TTL.as_secs().to_string();
    // `nullif` turns an empty string into SQL NULL, so the optional columns stay
    // nullable without needing a different parameter type.
    let user_agent = user_agent.unwrap_or_default();
    let ip = ip.unwrap_or_default();

    let row = runner
        .run_query_one(
            "insert into _sessions \
             (id, collection_id, user_id, token_hash, family_id, expires_at, user_agent, ip) \
             values ($1::text, nullif($2::text, ''), $3::text, $4::text, $5::text, \
                     now() + make_interval(secs => $6::text::int), \
                     nullif($7::text, ''), nullif($8::text, '')) \
             returning id, collection_id, user_id, family_id, expires_at, revoked_at",
            &[
                &id,
                &collection_id.to_string(),
                &user_id.to_string(),
                &token_hash,
                &family_id.to_string(),
                &seconds,
                &user_agent.to_string(),
                &ip.to_string(),
            ],
        )
        .await
        .map_err(db_error)?;

    Ok((token, from_row(&row)?))
}

/// The columns a session query selects.
const SESSION_COLUMNS: &str = "id, collection_id, user_id, family_id, expires_at, revoked_at";

/// Look a session up by its refresh token without touching it.
///
/// Rotation is destructive, so any check that has to come *before* it — does this token
/// belong to the collection in the URL? — has to be able to read the session without
/// consuming it. Otherwise a token sent to the wrong endpoint is consumed, the response
/// carrying its replacement is thrown away, and the user is logged out by a client bug.
pub async fn peek(runner: &dyn QueryRunner, token: &str) -> Result<Option<Session>> {
    let row = runner
        .run_query_opt(
            &format!("select {SESSION_COLUMNS} from _sessions where token_hash = $1::text"),
            &[&hash_token(token)],
        )
        .await
        .map_err(db_error)?;

    row.map(|row| from_row(&row)).transpose()
}

/// Exchange a refresh token for a new one, retiring the previous token.
pub async fn rotate(
    runner: &dyn QueryRunner,
    token: &str,
    user_agent: Option<&str>,
    ip: Option<&str>,
) -> Result<(String, Session)> {
    let hash = hash_token(token);

    // The claim is **one conditional statement**: only the request that flips `revoked_at`
    // from null mints the next token. Reading the session first and updating it afterwards
    // let two simultaneous uses both pass the check and both mint a token — which quietly
    // defeated the replay detection below, because the second use of a stolen token then
    // looked like an ordinary rotation.
    let claimed = runner
        .run_query_opt(
            &format!(
                "update _sessions set revoked_at = now() \
                 where token_hash = $1::text and revoked_at is null and expires_at > now() \
                 returning {SESSION_COLUMNS}"
            ),
            &[&hash],
        )
        .await
        .map_err(db_error)?;

    if let Some(row) = claimed {
        let session = from_row(&row)?;
        return issue_in_family(
            runner,
            &session.collection_id,
            &session.user_id,
            &session.family_id,
            user_agent,
            ip,
        )
        .await;
    }

    // Nothing was claimed, so say why. A token that exists but could not be claimed was
    // either revoked already — a replay — or has expired.
    let Some(row) = runner
        .run_query_opt(
            &format!("select {SESSION_COLUMNS} from _sessions where token_hash = $1::text"),
            &[&hash],
        )
        .await
        .map_err(db_error)?
    else {
        return Err(Error::unauthorized("the refresh token is not recognised"));
    };
    let session = from_row(&row)?;

    if session.revoked {
        // The token was already rotated, so this is a replay. Drop the family:
        // whoever holds the newer token loses it too, which is the safe outcome.
        let revoked = revoke_family(runner, &session.family_id).await?;
        tracing::warn!(
            family = %session.family_id,
            revoked,
            "a rotated refresh token was replayed; the session family was revoked"
        );
        return Err(Error::unauthorized(
            "this refresh token has already been used",
        ));
    }

    Err(Error::unauthorized("the refresh token has expired"))
}

/// Revoke every session in a family, returning how many were revoked.
pub async fn revoke_family(runner: &dyn QueryRunner, family_id: &str) -> Result<u64> {
    runner
        .run_execute(
            "update _sessions set revoked_at = now() \
             where family_id = $1::text and revoked_at is null",
            &[&family_id],
        )
        .await
        .map_err(db_error)
}

/// Revoke every session for an account, returning how many were revoked.
pub async fn revoke_for_user(
    runner: &dyn QueryRunner,
    collection_id: &str,
    user_id: &str,
) -> Result<u64> {
    runner
        .run_execute(
            // `is not distinct from` rather than `=`: a superuser's sessions have a
            // NULL collection id, and `collection_id = NULL` is never true, so the
            // equality form silently revoked nothing for them.
            "update _sessions set revoked_at = now() \
             where user_id = $1::text \
             and collection_id is not distinct from nullif($2::text, '') \
             and revoked_at is null",
            &[&user_id, &collection_id],
        )
        .await
        .map_err(db_error)
}

/// Delete sessions that are long past their expiry.
pub async fn purge_expired(runner: &dyn QueryRunner) -> Result<u64> {
    runner
        .run_execute("delete from _sessions where expires_at < now()", &[])
        .await
        .map_err(db_error)
}

fn from_row(row: &Row) -> Result<Session> {
    Ok(Session {
        id: row.get("id"),
        collection_id: row
            .get::<_, Option<String>>("collection_id")
            .unwrap_or_default(),
        user_id: row.get("user_id"),
        family_id: row.get("family_id"),
        expires_at: row.get("expires_at"),
        revoked: row.get::<_, Option<SystemTime>>("revoked_at").is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_url_safe() {
        let first = generate_token().expect("token");
        let second = generate_token().expect("token");

        assert_ne!(first, second);
        assert!(!first.contains('+') && !first.contains('/') && !first.contains('='));
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(&first)
                .unwrap()
                .len(),
            32
        );
    }

    #[test]
    fn hashing_is_stable_and_hides_the_token() {
        let token = generate_token().unwrap();
        let hash = hash_token(&token);

        assert_eq!(hash, hash_token(&token));
        assert_eq!(hash.len(), 64);
        assert!(!hash.contains(&token));
        assert_ne!(hash, hash_token("something else"));
    }
}
