//! Superusers (`_superusers`): administrators that live outside every
//! collection and bypass access rules.

use crate::password;
use crate::pg::{q, q_opt};
use crate::sessions;
use lazypock_core::id::new_id;
use lazypock_core::{Error, Result};
use tokio_postgres::{GenericClient, Row};

/// The collection name superusers are reported under (token `name`, expected-collection checks).
pub const COLLECTION: &str = "_superusers";

#[derive(Clone, PartialEq, Eq)]
pub struct Superuser {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub token_key: String,
}

impl std::fmt::Debug for Superuser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Superuser")
            .field("id", &self.id)
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

const COLUMNS: &str = "id, email, password_hash, token_key";

fn from_row(row: &Row) -> Superuser {
    Superuser {
        id: row.get(0),
        email: row.get(1),
        password_hash: row.get::<_, Option<String>>(2).unwrap_or_default(),
        token_key: row.get::<_, Option<String>>(3).unwrap_or_default(),
    }
}

pub async fn find_by_email<C: GenericClient>(client: &C, email: &str) -> Result<Option<Superuser>> {
    let row = q_opt(
        client,
        &format!("select {COLUMNS} from _superusers where lower(email) = lower($1::text)"),
        &[email.trim()],
    )
    .await?;
    Ok(row.as_ref().map(from_row))
}

pub async fn find_by_id<C: GenericClient>(client: &C, id: &str) -> Result<Option<Superuser>> {
    let row = q_opt(
        client,
        &format!("select {COLUMNS} from _superusers where id = $1::text"),
        &[id],
    )
    .await?;
    Ok(row.as_ref().map(from_row))
}

pub async fn count<C: GenericClient>(client: &C) -> Result<i64> {
    let rows = q(client, "select count(*) from _superusers", &[]).await?;
    Ok(rows[0].get(0))
}

/// A pragmatic address check: one `@`, something on both sides, a dot in the
/// domain, no whitespace or control characters, at most 254 bytes. (Deliverability
/// is for a verification email, not a regex.)
pub fn normalize_email(email: &str) -> Result<String> {
    let email = email.trim().to_lowercase();
    let ok = email.len() <= 254
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains('@')
        });
    if ok {
        Ok(email)
    } else {
        Err(Error::Validation("the email address is not valid".into()))
    }
}

/// Create a superuser. A duplicate email (case-insensitive) is a `Conflict`.
pub async fn create<C: GenericClient>(
    client: &C,
    email: &str,
    password: &str,
) -> Result<Superuser> {
    let email = normalize_email(email)?;
    let hash = password::hash(password)?;
    let token_key = sessions::generate_token()?;
    let id = new_id();
    q(
        client,
        "insert into _superusers (id, email, password_hash, token_key) values ($1::text, $2::text, $3::text, $4::text) returning id",
        &[&id, &email, &hash, &token_key],
    )
    .await?;
    Ok(Superuser {
        id,
        email,
        password_hash: hash,
        token_key,
    })
}

/// Change a password. Rotates the token key and revokes every session, so access
/// tokens and refresh tokens issued before the change stop working.
pub async fn set_password<C: GenericClient>(client: &C, id: &str, password: &str) -> Result<()> {
    let hash = password::hash(password)?;
    let token_key = sessions::generate_token()?;
    let rows = q(
        client,
        "update _superusers set password_hash = $2::text, token_key = $3::text, updated = now() where id = $1::text returning id",
        &[id, &hash, &token_key],
    )
    .await?;
    if rows.is_empty() {
        return Err(Error::NotFound("superuser".into()));
    }
    sessions::revoke_for_user(client, "", id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_normalisation() {
        assert_eq!(
            normalize_email("  Admin@Example.COM ").unwrap(),
            "admin@example.com"
        );
        for bad in [
            "",
            "a",
            "a@b",
            "@b.c",
            "a@.c",
            "a@b.",
            "a b@c.d",
            "a@b@c.d",
            "a@b\n.c",
            &format!("{}@x.io", "a".repeat(250)),
        ] {
            assert!(normalize_email(bad).is_err(), "{bad:?}");
        }
    }
}
