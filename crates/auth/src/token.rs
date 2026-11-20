//! Access tokens: Ed25519 (EdDSA) JWTs, signed and verified with `ring` directly.
//!
//! A JWT here is just `base64url(header).base64url(claims).base64url(signature)`,
//! so no JWT library is pulled in (the general-purpose ones bring RSA, P-256 and
//! P-384 stacks this never uses). The format is fixed on purpose:
//! - the algorithm is **always** EdDSA and the header is *checked*, never trusted
//!   to pick the algorithm, which closes `alg: none` / HS256-confusion attacks;
//! - `exp` is required and enforced (30 s leeway for clock skew);
//! - tokens longer than [`MAX_TOKEN_LEN`] are rejected before any parsing.

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use lazypock_core::{Error, Result};
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long an access token stays valid.
pub const ACCESS_TOKEN_TTL: Duration = Duration::from_secs(15 * 60);
pub const MAX_TOKEN_LEN: usize = 4096;
const LEEWAY_SECS: i64 = 30;
pub const ED25519_SEED_LEN: usize = 32;

/// What a token was issued for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Subject {
    /// A record in an auth collection.
    Auth,
    /// A superuser (bypasses access rules).
    Superuser,
}

impl Subject {
    pub fn is_superuser(self) -> bool {
        matches!(self, Self::Superuser)
    }
}

/// Claims carried by an access token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// Record or superuser id.
    pub sub: String,
    /// Collection id; empty for a superuser.
    pub col: String,
    /// Collection name; empty for a superuser.
    pub name: String,
    /// The account's email (for `@request.auth.email`).
    pub email: String,
    pub typ: Subject,
    /// Issued at / expiry, unix seconds.
    pub iat: i64,
    pub exp: i64,
    /// Signing key id.
    pub kid: String,
    /// The account's token key: changing the stored value invalidates every token
    /// issued for that account, without touching the signing key.
    pub tkey: String,
}

/// PKCS#8 v2 framing of an Ed25519 key: header, 32-byte seed, public-key tag,
/// 32-byte public key. Byte-for-byte what `ring::generate_pkcs8` writes, so a key
/// built from a seed and a generated one are interchangeable.
const PKCS8_HEADER: [u8; 16] = [
    0x30, 0x51, 0x02, 0x01, 0x01, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];
const PKCS8_PUBLIC_TAG: [u8; 3] = [0x81, 0x21, 0x00];

pub struct SigningKeys {
    kid: String,
    der: Vec<u8>,
    pair: Ed25519KeyPair,
    public_raw: [u8; 32],
}

impl std::fmt::Debug for SigningKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material.
        f.debug_struct("SigningKeys").field("kid", &self.kid).finish_non_exhaustive()
    }
}

fn unauthorized(msg: &str) -> Error {
    Error::Unauthorized(msg.to_string())
}

impl SigningKeys {
    pub fn generate() -> Result<Self> {
        let doc = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
            .map_err(|_| Error::Internal("cannot generate a signing key".into()))?;
        Self::from_der(doc.as_ref())
    }

    /// From a raw 32-byte Ed25519 seed (e.g. `openssl rand -base64 32`).
    pub fn from_seed(seed: &[u8]) -> Result<Self> {
        if seed.len() != ED25519_SEED_LEN {
            return Err(Error::Config(format!(
                "an Ed25519 signing key is {ED25519_SEED_LEN} bytes, got {}",
                seed.len()
            )));
        }
        let pair = Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|e| Error::Config(format!("the signing key seed is unusable: {e}")))?;
        let mut der = Vec::with_capacity(PKCS8_HEADER.len() + 32 + PKCS8_PUBLIC_TAG.len() + 32);
        der.extend_from_slice(&PKCS8_HEADER);
        der.extend_from_slice(seed);
        der.extend_from_slice(&PKCS8_PUBLIC_TAG);
        der.extend_from_slice(pair.public_key().as_ref());
        Self::from_der(&der)
    }

    pub fn from_der(der: &[u8]) -> Result<Self> {
        let pair = Ed25519KeyPair::from_pkcs8(der)
            .map_err(|e| Error::Config(format!("the signing key is not a valid Ed25519 key: {e}")))?;
        let public_raw: [u8; 32] = pair
            .public_key()
            .as_ref()
            .try_into()
            .map_err(|_| Error::Config("the signing key has an unexpected public key length".into()))?;
        Ok(SigningKeys { kid: key_id(&public_raw), der: der.to_vec(), pair, public_raw })
    }

    /// PKCS#8 document, for storage.
    pub fn to_der(&self) -> &[u8] {
        &self.der
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// The public half as a JWK, for `/.well-known/jwks.json`-style endpoints.
    pub fn jwk(&self) -> Value {
        json!({
            "kty": "OKP", "crv": "Ed25519", "alg": "EdDSA", "use": "sig",
            "kid": self.kid, "x": B64.encode(self.public_raw),
        })
    }

    pub fn sign(&self, claims: &Claims) -> Result<String> {
        let header = json!({ "alg": "EdDSA", "typ": "JWT", "kid": self.kid });
        let payload = serde_json::to_vec(claims)
            .map_err(|e| Error::Internal(format!("cannot encode the access token: {e}")))?;
        let signing_input = format!("{}.{}", B64.encode(header.to_string()), B64.encode(payload));
        let signature = self.pair.sign(signing_input.as_bytes());
        Ok(format!("{signing_input}.{}", B64.encode(signature.as_ref())))
    }

    pub fn verify(&self, token: &str) -> Result<Claims> {
        let invalid = || unauthorized("the access token is not valid");
        if token.len() > MAX_TOKEN_LEN {
            return Err(invalid());
        }
        let mut parts = token.split('.');
        let (Some(h), Some(p), Some(s), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            return Err(invalid());
        };

        let header: Value = serde_json::from_slice(&B64.decode(h).map_err(|_| invalid())?).map_err(|_| invalid())?;
        if header.get("alg").and_then(Value::as_str) != Some("EdDSA") {
            return Err(invalid());
        }
        if header.get("kid").and_then(Value::as_str).is_some_and(|k| k != self.kid) {
            return Err(unauthorized("the access token was signed with a different key"));
        }

        let signature = B64.decode(s).map_err(|_| invalid())?;
        UnparsedPublicKey::new(&ED25519, &self.public_raw)
            .verify(format!("{h}.{p}").as_bytes(), &signature)
            .map_err(|_| invalid())?;

        let claims: Claims = serde_json::from_slice(&B64.decode(p).map_err(|_| invalid())?).map_err(|_| invalid())?;
        if now_secs()? > claims.exp.saturating_add(LEEWAY_SECS) {
            return Err(unauthorized("the access token has expired"));
        }
        Ok(claims)
    }

    pub fn claims_for(
        &self,
        subject: Subject,
        id: &str,
        email: &str,
        collection_id: &str,
        collection_name: &str,
        token_key: &str,
    ) -> Result<Claims> {
        let iat = now_secs()?;
        Ok(Claims {
            sub: id.to_string(),
            col: collection_id.to_string(),
            name: collection_name.to_string(),
            email: email.to_string(),
            typ: subject,
            iat,
            exp: iat + ACCESS_TOKEN_TTL.as_secs() as i64,
            kid: self.kid.clone(),
            tkey: token_key.to_string(),
        })
    }
}

fn now_secs() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|_| Error::Internal("the system clock is before the Unix epoch".into()))
}

fn key_id(public_raw: &[u8; 32]) -> String {
    Sha256::digest(public_raw)[..8].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(keys: &SigningKeys) -> Claims {
        keys.claims_for(Subject::Auth, "u1", "a@b.c", "col1", "users", "tk").unwrap()
    }

    fn forge(header: Value, claims: &Claims, signer: &SigningKeys) -> String {
        let input = format!("{}.{}", B64.encode(header.to_string()), B64.encode(serde_json::to_vec(claims).unwrap()));
        let sig = signer.pair.sign(input.as_bytes());
        format!("{input}.{}", B64.encode(sig.as_ref()))
    }

    #[test]
    fn round_trip() {
        let keys = SigningKeys::generate().unwrap();
        let c = claims(&keys);
        let back = keys.verify(&keys.sign(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.exp - back.iat, ACCESS_TOKEN_TTL.as_secs() as i64);
        assert!(!back.typ.is_superuser());
    }

    #[test]
    fn another_key_cannot_verify() {
        let (a, b) = (SigningKeys::generate().unwrap(), SigningKeys::generate().unwrap());
        let token = a.sign(&claims(&a)).unwrap();
        assert!(b.verify(&token).is_err());
        // Same signature bytes under the other key's id: still fails (signature check).
        let c = claims(&b);
        assert!(b.verify(&forge(json!({"alg": "EdDSA", "kid": b.kid}), &c, &a)).is_err());
    }

    #[test]
    fn tampered_and_malformed_tokens_are_rejected() {
        let keys = SigningKeys::generate().unwrap();
        let token = keys.sign(&claims(&keys)).unwrap();
        let mut parts: Vec<&str> = token.split('.').collect();
        let other = keys.sign(&Claims { sub: "admin".into(), ..claims(&keys) }).unwrap();
        let other_payload = other.split('.').nth(1).unwrap().to_string();
        parts[1] = &other_payload;
        assert!(keys.verify(&parts.join(".")).is_err(), "payload swapped, signature kept");
        for bad in ["", "a", "a.b", "a.b.c", "a.b.c.d", "...", &format!("{token}.x"), &"x".repeat(MAX_TOKEN_LEN + 1)] {
            assert!(keys.verify(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_algorithm_is_never_taken_from_the_token() {
        let keys = SigningKeys::generate().unwrap();
        let c = claims(&keys);
        for alg in ["none", "HS256", "RS256", "ES256", "eddsa", ""] {
            let t = forge(json!({"alg": alg, "kid": keys.kid}), &c, &keys);
            assert!(keys.verify(&t).is_err(), "alg {alg:?} must be refused");
        }
        let no_alg = forge(json!({"kid": keys.kid}), &c, &keys);
        assert!(keys.verify(&no_alg).is_err());
        // `alg: none` with an empty signature
        let input = format!("{}.{}", B64.encode(r#"{"alg":"none"}"#), B64.encode(serde_json::to_vec(&c).unwrap()));
        assert!(keys.verify(&format!("{input}.")).is_err());
    }

    #[test]
    fn expiry_is_enforced_with_leeway() {
        let keys = SigningKeys::generate().unwrap();
        let mut c = claims(&keys);
        c.exp = now_secs().unwrap() - 3600;
        let e = keys.verify(&keys.sign(&c).unwrap()).unwrap_err();
        assert!(matches!(e, Error::Unauthorized(ref m) if m.contains("expired")), "{e}");
        c.exp = now_secs().unwrap() - 5; // inside the 30 s leeway
        assert!(keys.verify(&keys.sign(&c).unwrap()).is_ok());
    }

    #[test]
    fn a_token_without_exp_is_refused() {
        let keys = SigningKeys::generate().unwrap();
        let payload = json!({"sub":"u","col":"","name":"","email":"","typ":"auth","iat":1,"kid":keys.kid,"tkey":""});
        let input = format!("{}.{}", B64.encode(json!({"alg":"EdDSA"}).to_string()), B64.encode(payload.to_string()));
        let t = format!("{input}.{}", B64.encode(keys.pair.sign(input.as_bytes()).as_ref()));
        assert!(keys.verify(&t).is_err());
    }

    #[test]
    fn keys_survive_der_and_seed_round_trips() {
        let keys = SigningKeys::generate().unwrap();
        let again = SigningKeys::from_der(keys.to_der()).unwrap();
        assert_eq!(keys.kid(), again.kid());
        assert!(again.verify(&keys.sign(&claims(&keys)).unwrap()).is_ok());

        let seed = [7u8; 32];
        let (a, b) = (SigningKeys::from_seed(&seed).unwrap(), SigningKeys::from_seed(&seed).unwrap());
        assert_eq!(a.kid(), b.kid());
        assert_eq!(a.sign(&claims(&a)).unwrap().split('.').nth(2), a.sign(&claims(&a)).unwrap().split('.').nth(2), "Ed25519 is deterministic");
        assert!(SigningKeys::from_seed(&[1u8; 31]).is_err());
        assert!(SigningKeys::from_der(b"not a key").is_err());
    }

    #[test]
    fn the_jwk_is_public_only_and_debug_hides_keys() {
        let keys = SigningKeys::generate().unwrap();
        let jwk = keys.jwk();
        assert_eq!(jwk["kty"], "OKP");
        assert!(jwk.get("d").is_none());
        let dump = format!("{keys:?} {jwk}");
        assert!(!dump.contains(&B64.encode(keys.to_der())));
        assert_eq!(B64.decode(jwk["x"].as_str().unwrap()).unwrap().len(), 32);
    }
}
