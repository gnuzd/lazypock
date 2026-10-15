//! Encrypting a credential that is stored in the database.
//!
//! A superuser can configure storage from the Studio, and that configuration — including
//! the S3 secret — is kept in `_settings`. Anything in `_settings` is also in every
//! backup, every replica and every `pg_dump`, so the secret is stored **encrypted**, with
//! a key that only the environment holds:
//!
//! ```bash
//! LAZYPOCK_SECRET_KEY=$(openssl rand -hex 32)
//! ```
//!
//! That is the whole point of the split: the database alone is not enough to recover the
//! credential, and a leaked backup is a much smaller problem than a leaked secret. The
//! flip side is stated where it matters: **losing `LAZYPOCK_SECRET_KEY` means the stored
//! secret cannot be read again** — a saved one has to be entered again, and the file
//! backend keeps working meanwhile.
//!
//! Saving a secret without a key configured is refused rather than stored in the clear.
//! AES-256-GCM from `ring` — the same crate the token layer already uses, so this adds no
//! dependency to the build — with a random nonce per value and the field name as
//! associated data, so a ciphertext cannot be moved between fields unnoticed.
//!
//! The stored form is `v1:<nonce>:<ciphertext>`, hex, so a future scheme is recognised
//! rather than guessed at.

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};

use lazypock_core::error::{Error, Result};

/// What an encrypted value starts with.
const PREFIX: &str = "v1";

/// The environment variable that holds the key.
pub const KEY_VARIABLE: &str = "LAZYPOCK_SECRET_KEY";

/// How long the key must be, in hex characters (32 bytes).
const KEY_HEX_LENGTH: usize = 64;

/// The key that encrypts stored credentials.
pub struct Secrets {
    key: LessSafeKey,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key.
        f.write_str("Secrets { key: [redacted] }")
    }
}

impl Secrets {
    /// Read the key from the environment, if it is set.
    ///
    /// `None` is not an error: a deployment that has nothing to store this way does not
    /// need a key. It only becomes a problem when a secret is about to be saved.
    pub fn from_env() -> Result<Option<Self>> {
        match std::env::var(KEY_VARIABLE) {
            Err(_) => Ok(None),
            Ok(raw) if raw.trim().is_empty() => Ok(None),
            Ok(raw) => Self::parse(&raw).map(Some),
        }
    }

    /// Parse a key: 32 bytes, hex, for AES-256.
    pub fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.len() != KEY_HEX_LENGTH {
            return Err(Error::config(format!(
                "{KEY_VARIABLE} must be {KEY_HEX_LENGTH} hex characters (32 bytes), got {} \
                 — generate one with `openssl rand -hex 32`",
                trimmed.len()
            )));
        }

        let bytes = decode_hex(trimmed).ok_or_else(|| {
            Error::config(format!(
                "{KEY_VARIABLE} must be hex characters only, e.g. `openssl rand -hex 32`"
            ))
        })?;

        let key = UnboundKey::new(&AES_256_GCM, &bytes)
            .map_err(|_| Error::config(format!("{KEY_VARIABLE} is not a usable AES-256 key")))?;
        Ok(Self {
            key: LessSafeKey::new(key),
        })
    }

    /// Encrypt a value for storage, bound to the field it belongs to.
    pub fn encrypt(&self, field: &str, plaintext: &str) -> Result<String> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| Error::internal("the system random source is unavailable"))?;

        let mut buffer = plaintext.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::from(field.as_bytes()),
                &mut buffer,
            )
            .map_err(|_| Error::internal("cannot encrypt the stored secret"))?;

        Ok(format!(
            "{PREFIX}:{}:{}",
            encode_hex(&nonce_bytes),
            encode_hex(&buffer)
        ))
    }

    /// Decrypt a value, or say why it cannot be.
    pub fn decrypt(&self, field: &str, stored: &str) -> Result<String> {
        let mut parts = stored.split(':');
        let (Some(prefix), Some(nonce), Some(ciphertext), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(Error::config(format!(
                "the stored {field} is not in the expected `{PREFIX}:<nonce>:<ciphertext>` form"
            )));
        };
        if prefix != PREFIX {
            return Err(Error::config(format!(
                "the stored {field} uses an unknown encryption version {prefix:?}"
            )));
        }

        let nonce_bytes = decode_hex(nonce)
            .ok_or_else(|| Error::config(format!("the stored {field} has a malformed nonce")))?;
        let nonce_bytes: [u8; NONCE_LEN] = nonce_bytes.try_into().map_err(|_| {
            Error::config(format!(
                "the stored {field} has a nonce of the wrong length"
            ))
        })?;

        let mut buffer = decode_hex(ciphertext)
            .ok_or_else(|| Error::config(format!("the stored {field} is not hex")))?;

        let plaintext = self
            .key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::from(field.as_bytes()),
                &mut buffer,
            )
            .map_err(|_| {
                Error::config(format!(
                    "the stored {field} cannot be decrypted: {KEY_VARIABLE} does not match \
                     the key it was saved with"
                ))
            })?;

        String::from_utf8(plaintext.to_vec())
            .map_err(|_| Error::config(format!("the stored {field} is not valid UTF-8")))
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for pair in bytes.chunks(2) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        out.push((high * 16 + low) as u8);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secrets() -> Secrets {
        Secrets::parse(&"ab".repeat(32)).expect("a key")
    }

    #[test]
    fn a_value_round_trips() {
        let secrets = secrets();
        let stored = secrets
            .encrypt("secret_access_key", "s3cret")
            .expect("encrypt");
        assert!(stored.starts_with("v1:"), "{stored}");
        assert!(
            !stored.contains("s3cret"),
            "the plaintext must not survive: {stored}"
        );
        assert_eq!(
            secrets
                .decrypt("secret_access_key", &stored)
                .expect("decrypt"),
            "s3cret"
        );
    }

    #[test]
    fn the_same_value_encrypts_differently_every_time() {
        let secrets = secrets();
        let first = secrets
            .encrypt("secret_access_key", "s3cret")
            .expect("encrypt");
        let second = secrets
            .encrypt("secret_access_key", "s3cret")
            .expect("encrypt");
        assert_ne!(
            first, second,
            "a random nonce per value: {first} vs {second}"
        );
        assert_eq!(
            secrets
                .decrypt("secret_access_key", &first)
                .expect("decrypt"),
            "s3cret"
        );
    }

    #[test]
    fn a_ciphertext_cannot_be_moved_to_another_field() {
        // The field name is the associated data, so a value lifted from one field into
        // another fails to open rather than being accepted.
        let secrets = secrets();
        let stored = secrets
            .encrypt("secret_access_key", "s3cret")
            .expect("encrypt");
        assert!(secrets.decrypt("smtp_password", &stored).is_err());
    }

    #[test]
    fn another_key_cannot_read_it_and_says_which_variable() {
        let stored = secrets()
            .encrypt("secret_access_key", "s3cret")
            .expect("encrypt");
        let other = Secrets::parse(&"cd".repeat(32)).expect("a key");
        let err = other
            .decrypt("secret_access_key", &stored)
            .expect_err("refused");
        assert!(err.message().contains(KEY_VARIABLE), "{}", err.message());
    }

    #[test]
    fn a_malformed_key_is_refused_with_the_command_to_make_one() {
        for bad in ["", "abc", &"zz".repeat(32)] {
            let err = Secrets::parse(bad).expect_err("refused");
            assert!(
                err.message().contains("openssl rand -hex 32"),
                "{}",
                err.message()
            );
        }
    }

    #[test]
    fn a_malformed_stored_value_is_refused() {
        let secrets = secrets();
        for bad in [
            "",
            "s3cret",
            "v2:00:00",
            "v1:nothex:00",
            "v1:00",
            "v1:00:00:00",
        ] {
            assert!(
                secrets.decrypt("secret_access_key", bad).is_err(),
                "{bad} should not decrypt"
            );
        }
    }

    #[test]
    fn an_empty_environment_means_no_key_rather_than_a_broken_one() {
        // `from_env` is what the boot path calls; an unset or blank variable is "nothing
        // to store", not a misconfiguration.
        let previous = std::env::var(KEY_VARIABLE).ok();
        std::env::remove_var(KEY_VARIABLE);
        assert!(Secrets::from_env().expect("unset").is_none());

        std::env::set_var(KEY_VARIABLE, "   ");
        assert!(Secrets::from_env().expect("blank").is_none());

        std::env::set_var(KEY_VARIABLE, "ab".repeat(32));
        assert!(Secrets::from_env().expect("set").is_some());

        match previous {
            Some(value) => std::env::set_var(KEY_VARIABLE, value),
            None => std::env::remove_var(KEY_VARIABLE),
        }
    }
}
