//! Superuser accounts.
//!
//! Deliberately separate from auth collections: a superuser is not a record, it
//! cannot be created through the records API, and it bypasses access rules.

use std::time::SystemTime;

use lazypock_core::error::{Error, Result};
use lazypock_db::pool::db_error;
use lazypock_db::runner::QueryRunner;
use lazypock_db::tokio_postgres::Row;

use crate::password;
use crate::sessions;

/// A superuser account, without the password hash when one is not needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Superuser {
    /// Record id.
    pub id: String,
    /// Lowercased email address.
    pub email: String,
    /// argon2id hash of the password.
    pub password_hash: String,
    /// Rotating the stored value invalidates every issued token at once.
    pub token_key: String,
    /// Creation time.
    pub created: SystemTime,
    /// Last change.
    pub updated: SystemTime,
}

/// Create a superuser.
pub async fn create(runner: &dyn QueryRunner, email: &str, pass: &str) -> Result<Superuser> {
    let email = normalise_email(email)?;
    let password_hash = password::hash(pass)?;
    let id = lazypock_core::id::new_id();
    let token_key = sessions::generate_token()?;

    let row = runner
        .run_query_one(
            "insert into _superusers (id, email, password_hash, token_key) \
             values ($1::text, $2::text, $3::text, $4::text) \
             returning id, email, password_hash, token_key, created, updated",
            &[&id, &email, &password_hash, &token_key],
        )
        .await
        .map_err(|err| {
            if err
                .as_db_error()
                .is_some_and(|db| db.code().code() == "23505")
            {
                Error::conflict(format!(
                    "a superuser with the email {email:?} already exists"
                ))
            } else {
                db_error(err)
            }
        })?;

    tracing::info!(email = %email, "superuser created");
    Ok(from_row(&row))
}

/// Find a superuser by id.
pub async fn find_by_id(runner: &dyn QueryRunner, id: &str) -> Result<Option<Superuser>> {
    let row = runner
        .run_query_opt(
            "select id, email, password_hash, token_key, created, updated \
             from _superusers where id = $1::text",
            &[&id],
        )
        .await
        .map_err(db_error)?;
    Ok(row.as_ref().map(from_row))
}

/// Find a superuser by email, case-insensitively.
pub async fn find_by_email(runner: &dyn QueryRunner, email: &str) -> Result<Option<Superuser>> {
    let row = runner
        .run_query_opt(
            "select id, email, password_hash, token_key, created, updated \
             from _superusers where lower(email) = lower($1::text)",
            &[&email.trim().to_string()],
        )
        .await
        .map_err(db_error)?;
    Ok(row.as_ref().map(from_row))
}

/// Number of superusers, for first-boot checks.
pub async fn count(runner: &dyn QueryRunner) -> Result<i64> {
    let row = runner
        .run_query_one("select count(*) from _superusers", &[])
        .await
        .map_err(db_error)?;
    Ok(row.get(0))
}

/// Replace a superuser's password, end its sessions and invalidate its tokens.
pub async fn set_password(runner: &dyn QueryRunner, id: &str, pass: &str) -> Result<()> {
    let password_hash = password::hash(pass)?;
    let token_key = sessions::generate_token()?;
    let updated = runner
        .run_execute(
            "update _superusers set password_hash = $1::text, token_key = $2::text, \
             updated = now() where id = $3::text",
            &[&password_hash, &token_key, &id.to_string()],
        )
        .await
        .map_err(db_error)?;
    if updated == 0 {
        return Err(Error::not_found("no such superuser"));
    }

    // A password change is usually a response to a suspected compromise, so it
    // ends every session rather than leaving refresh tokens usable.
    let ended = sessions::revoke_for_user(runner, "", id).await?;
    tracing::info!(revoked = ended, "superuser password changed");
    Ok(())
}

/// Trim and lowercase an email, refusing anything that is clearly not one.
fn normalise_email(email: &str) -> Result<String> {
    let email = email.trim().to_ascii_lowercase();
    if email.is_empty()
        || !email.contains('@')
        || email.len() > 320
        || email.chars().any(char::is_whitespace)
    {
        return Err(Error::invalid(format!(
            "{email:?} is not a valid email address"
        )));
    }
    let (local, domain) = email.split_once('@').expect("checked");
    if local.is_empty() || !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.')
    {
        return Err(Error::invalid(format!(
            "{email:?} is not a valid email address"
        )));
    }
    Ok(email)
}

fn from_row(row: &Row) -> Superuser {
    Superuser {
        id: row.get("id"),
        email: row.get("email"),
        password_hash: row.get("password_hash"),
        token_key: row.get("token_key"),
        created: row.get("created"),
        updated: row.get("updated"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_normalised_and_validated() {
        assert_eq!(
            normalise_email("  Admin@Example.COM ").unwrap(),
            "admin@example.com"
        );

        for bad in [
            "",
            "   ",
            "not-an-email",
            "@example.com",
            "a@b",
            "a b@example.com",
            "a@.com",
        ] {
            assert!(normalise_email(bad).is_err(), "{bad:?} should be refused");
        }
    }
}
