//! Password hashing: Argon2id with the crate's recommended defaults and a random
//! salt per hash. The stored value is a PHC string (`$argon2id$v=19$m=...`), which
//! carries its own parameters, so raising the defaults later does not invalidate
//! existing hashes.

use argon2::password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use lazypock_core::{Error, Result};

pub const MIN_PASSWORD_LEN: usize = 8;
/// Upper bound so a request cannot make us hash megabytes.
pub const MAX_PASSWORD_LEN: usize = 1024;

/// Hash a new password. Rejects passwords outside the length bounds (400).
pub fn hash(password: &str) -> Result<String> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(Error::Validation(format!(
            "the password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    if password.len() > MAX_PASSWORD_LEN {
        return Err(Error::Validation(format!(
            "the password must be at most {MAX_PASSWORD_LEN} characters"
        )));
    }
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| Error::Internal(format!("cannot hash the password: {e}")))
}

/// Check a password against a stored hash. Never errors: anything that is not a
/// clean match (bad hash, oversize input) is simply `false`.
pub fn verify(password: &str, stored: &str) -> bool {
    if password.len() > MAX_PASSWORD_LEN {
        return false;
    }
    let Ok(parsed) = stored.parse::<PasswordHash>() else {
        tracing::warn!("a stored password hash could not be parsed; treating the login as failed");
        return false;
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

/// `hash` off the async worker threads (Argon2 is tens of milliseconds of CPU).
pub async fn hash_blocking(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || hash(&password))
        .await
        .map_err(|_| Error::Internal("the password hashing task failed".into()))?
}

/// `verify` off the async worker threads.
pub async fn verify_blocking(password: String, stored: String) -> bool {
    tokio::task::spawn_blocking(move || verify(&password, &stored)).await.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_verify_and_are_salted() {
        let a = hash("correct horse").unwrap();
        let b = hash("correct horse").unwrap();
        assert!(a.starts_with("$argon2id$"), "{a}");
        assert_ne!(a, b, "every hash has its own salt");
        assert!(verify("correct horse", &a) && verify("correct horse", &b));
        assert!(!verify("correct horsf", &a));
        assert!(!verify("", &a));
    }

    #[test]
    fn length_bounds() {
        assert!(matches!(hash("short"), Err(Error::Validation(_))));
        assert!(matches!(hash(&"x".repeat(MAX_PASSWORD_LEN + 1)), Err(Error::Validation(_))));
        assert!(hash(&"x".repeat(MAX_PASSWORD_LEN)).is_ok());
        let h = hash("correct horse").unwrap();
        assert!(!verify(&"x".repeat(MAX_PASSWORD_LEN + 1), &h));
    }

    #[test]
    fn unparsable_hashes_never_verify() {
        for stored in ["", "plaintext", "$argon2id$garbage", "$2b$12$notargon"] {
            assert!(!verify("correct horse", stored), "{stored:?}");
        }
    }
}
