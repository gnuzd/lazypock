//! Record identifier generation.
//!
//! IDs match PocketBase: [`ID_LEN`] characters drawn uniformly from
//! `[a-zA-Z0-9]`, generated with the OS CSPRNG. They are URL-safe, index
//! friendly, and not guessable. Like PocketBase they are *not* monotonic:
//! ordering uses `created`/`updated`, never the id.

/// Number of characters in a generated id.
pub const ID_LEN: usize = 15;

/// Alphabet used for IDs, in a fixed order so the mapping is deterministic.
const ALPHABET: &[u8; 62] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// Largest multiple of 62 that fits in a `u8`; bytes at or above this are
/// rejected to keep the distribution uniform (no modulo bias).
const REJECTION_THRESHOLD: u8 = 62 * 4; // 248

/// Generate a new random record id.
///
/// # Panics
///
/// Panics if the operating system entropy source is unavailable. That is a
/// non-recoverable environment failure and silently emitting predictable ids
/// would be worse.
pub fn new_id() -> String {
    let mut out = String::with_capacity(ID_LEN);
    let mut buf = [0u8; 32];
    while out.len() < ID_LEN {
        getrandom::fill(&mut buf).expect("OS random number generator unavailable");
        for byte in buf {
            if byte >= REJECTION_THRESHOLD {
                continue;
            }
            out.push(ALPHABET[(byte % 62) as usize] as char);
            if out.len() == ID_LEN {
                break;
            }
        }
    }
    out
}

/// Whether `candidate` is a syntactically valid lazypock id.
pub fn is_valid_id(candidate: &str) -> bool {
    candidate.len() == ID_LEN && candidate.bytes().all(|b| ALPHABET.contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn generated_id_has_expected_length_and_alphabet() {
        for _ in 0..1_000 {
            let id = new_id();
            assert_eq!(id.len(), ID_LEN);
            assert!(is_valid_id(&id), "unexpected id: {id}");
        }
    }

    #[test]
    fn generated_ids_are_effectively_unique() {
        let ids: HashSet<String> = (0..10_000).map(|_| new_id()).collect();
        assert_eq!(ids.len(), 10_000, "id collision within 10k draws");
    }

    #[test]
    fn alphabet_is_uniformly_reachable() {
        // A wide sample must touch every character, catching an alphabet or
        // modulo bug that would otherwise only bias the distribution.
        let seen: HashSet<char> = (0..2_000)
            .flat_map(|_| new_id().chars().collect::<Vec<_>>())
            .collect();
        assert_eq!(seen.len(), 62, "not all alphabet characters were produced");
    }

    #[test]
    fn validation_rejects_wrong_length_and_charset() {
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("short"));
        assert!(!is_valid_id(&"a".repeat(ID_LEN + 1)));
        assert!(!is_valid_id("!!!!!!!!!!!!!!!"));
        assert!(is_valid_id(&"a".repeat(ID_LEN)));
    }
}
