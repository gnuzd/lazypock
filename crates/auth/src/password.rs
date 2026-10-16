//! Password hashing (argon2id).
//!
//! Verification never returns an error: a wrong password, a malformed stored
//! hash, or an unknown algorithm all simply fail, so a caller cannot mistake a
//! parse problem for a successful login.

use argon2::password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use lazypock_core::error::{Error, Result};

/// Shortest password accepted when one is set.
pub const MIN_PASSWORD_LEN: usize = 8;

/// Hash a password for storage.
pub fn hash(password: &str) -> Result<String> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(Error::invalid(format!(
            "the password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    // The salt is generated per call by the hasher itself.
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|err| Error::internal(format!("cannot hash the password: {err}")))
}

/// Check a password against a stored hash.
///
/// The comparison happens inside argon2, so it is constant time with respect to
/// the stored hash.
pub fn verify(password: &str, stored: &str) -> bool {
    let Ok(parsed) = stored.parse::<PasswordHash>() else {
        tracing::warn!("a stored password hash could not be parsed; treating the login as failed");
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_verify_and_are_salted() {
        let first = hash("correct horse battery").expect("hash");
        let second = hash("correct horse battery").expect("hash");

        // A fresh salt every time, so the same password never yields the same hash.
        assert_ne!(first, second);
        assert!(first.starts_with("$argon2"), "{first}");

        assert!(verify("correct horse battery", &first));
        assert!(!verify("wrong password", &first));
    }

    #[test]
    fn short_passwords_are_refused() {
        let err = hash("short").expect_err("must be refused");
        assert!(err.message().contains("at least"), "{err}");
        assert!(hash(&"a".repeat(MIN_PASSWORD_LEN)).is_ok());
    }

    #[test]
    fn malformed_stored_hashes_fail_closed() {
        for stored in [
            "",
            "not a hash",
            "$argon2id$garbage",
            "$2b$12$abcdefghijklmnopqrstuv",
        ] {
            assert!(!verify("anything", stored), "{stored:?} should not verify");
        }
    }

    #[test]
    fn the_hash_is_unicode_safe() {
        let hash = hash("contraseña-ñ-🔐").expect("hash");
        assert!(verify("contraseña-ñ-🔐", &hash));
        assert!(!verify("contraseña-ñ", &hash));
    }
}
