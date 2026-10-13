//! Access tokens: Ed25519 (EdDSA) JWTs and their signing keys.
//!
//! An asymmetric key is used so a token can be verified from the public key
//! alone; the public half is published as a JWK so other services can check a
//! token without holding the signing secret.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use lazypock_core::error::{Error, Result};
use ring::signature::KeyPair;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// How long an access token stays valid.
pub const ACCESS_TOKEN_TTL: Duration = Duration::from_secs(15 * 60);

/// What a token was issued for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Subject {
    /// A record in an auth collection.
    Auth,
    /// A superuser.
    Superuser,
}

impl Subject {
    /// Whether this subject bypasses access rules.
    pub fn is_superuser(self) -> bool {
        matches!(self, Self::Superuser)
    }
}

/// Claims carried by an access token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// Subject id: the record or superuser id.
    pub sub: String,
    /// Collection id, empty for a superuser.
    pub col: String,
    /// Collection name, empty for a superuser.
    pub name: String,
    /// What the token is for.
    pub typ: Subject,
    /// Issued at, unix seconds.
    pub iat: i64,
    /// Expiry, unix seconds.
    pub exp: i64,
    /// Signing key id, for rotation.
    pub kid: String,
    /// The account's token key.
    ///
    /// Bumping the stored value invalidates every token already handed out for
    /// that account without touching the signing key.
    pub tkey: String,
}

/// An Ed25519 key pair used to sign and verify access tokens.
pub struct SigningKeys {
    kid: String,
    der: Vec<u8>,
    encoding: EncodingKey,
    decoding: DecodingKey,
    public_raw: [u8; 32],
}

impl std::fmt::Debug for SigningKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the private half.
        f.debug_struct("SigningKeys")
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

impl SigningKeys {
    /// Generate a fresh key pair.
    pub fn generate() -> Result<Self> {
        let document =
            ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                .map_err(|_| Error::internal("cannot generate a signing key"))?;
        Self::from_der(document.as_ref())
    }

    /// Build from a PKCS#8 DER document.
    pub fn from_der(der: &[u8]) -> Result<Self> {
        let pair = ring::signature::Ed25519KeyPair::from_pkcs8(der).map_err(|err| {
            Error::config(format!("the signing key is not a valid Ed25519 key: {err}"))
        })?;
        let public_raw: [u8; 32] =
            pair.public_key().as_ref().try_into().map_err(|_| {
                Error::config("the signing key has an unexpected public key length")
            })?;

        // Verify from the same encoding the JWK publishes, so the published key
        // and the one used to check tokens can never disagree.
        let jwk_x = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_raw);

        Ok(Self {
            kid: key_id(&public_raw),
            der: der.to_vec(),
            encoding: EncodingKey::from_ed_der(der),
            decoding: DecodingKey::from_ed_components(&jwk_x).map_err(|err| {
                Error::config(format!("the signing key's public half is unusable: {err}"))
            })?,
            public_raw,
        })
    }

    /// The PKCS#8 DER of the private key, for storage.
    pub fn to_der(&self) -> &[u8] {
        &self.der
    }

    /// Key id, derived from the public key so a rotated key gets a new id.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// The public JWK, for publishing.
    pub fn jwk(&self) -> Value {
        json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "alg": "EdDSA",
            "use": "sig",
            "kid": self.kid,
            "x": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.public_raw),
        })
    }

    /// Sign claims into a compact JWT.
    pub fn sign(&self, claims: &Claims) -> Result<String> {
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(self.kid.clone());
        jsonwebtoken::encode(&header, claims, &self.encoding)
            .map_err(|err| Error::internal(format!("cannot sign the access token: {err}")))
    }

    /// Verify a token and return its claims.
    ///
    /// The algorithm is pinned to EdDSA and the expiry is checked, so a token
    /// signed with another algorithm, or one that has expired, is rejected.
    pub fn verify(&self, token: &str) -> Result<Claims> {
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.validate_exp = true;
        // A little slack for clock skew between nodes, but small enough that an
        // expired token stops working promptly.
        validation.leeway = 30;
        validation.required_spec_claims = ["exp".to_string()].into_iter().collect();

        let data = jsonwebtoken::decode::<Claims>(token, &self.decoding, &validation)
            .map_err(|err| Error::unauthorized(format!("the access token is not valid: {err}")))?;
        Ok(data.claims)
    }

    /// Build the claims for a new access token.
    pub fn claims_for(
        &self,
        subject: Subject,
        id: &str,
        collection_id: &str,
        collection_name: &str,
        token_key: &str,
    ) -> Result<Claims> {
        let issued = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::internal("the system clock is before the Unix epoch"))?
            .as_secs();
        Ok(Claims {
            sub: id.to_string(),
            col: collection_id.to_string(),
            name: collection_name.to_string(),
            typ: subject,
            iat: issued as i64,
            exp: (issued + ACCESS_TOKEN_TTL.as_secs()) as i64,
            kid: self.kid.clone(),
            tkey: token_key.to_string(),
        })
    }
}

/// A short, stable id for a public key.
fn key_id(public_raw: &[u8; 32]) -> String {
    let digest = Sha256::digest(public_raw);
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(keys: &SigningKeys) -> Claims {
        keys.claims_for(Subject::Auth, "user1", "col1", "users", "tk1")
            .expect("claims")
    }

    #[test]
    fn tokens_round_trip_and_verify() {
        let keys = SigningKeys::generate().expect("generate");
        let claims = claims(&keys);
        let token = keys.sign(&claims).expect("sign");

        // Compact JWS: three dot-separated parts.
        assert_eq!(token.matches('.').count(), 2, "{token}");

        let decoded = keys.verify(&token).expect("verify");
        assert_eq!(decoded, claims);
        assert_eq!(decoded.kid, keys.kid());
    }

    #[test]
    fn a_different_key_cannot_verify() {
        let signer = SigningKeys::generate().expect("generate");
        let other = SigningKeys::generate().expect("generate");
        let token = signer.sign(&claims(&signer)).expect("sign");

        let err = other.verify(&token).expect_err("must not verify");
        assert_eq!(err.status(), 401, "{err}");
    }

    #[test]
    fn tampered_and_malformed_tokens_are_rejected() {
        let keys = SigningKeys::generate().expect("generate");
        let token = keys.sign(&claims(&keys)).expect("sign");

        let mut parts: Vec<&str> = token.split('.').collect();
        // Flip the payload to something else, keeping the signature.
        parts[1] = "eyJzdWIiOiJoYWNrZXIifQ";
        let tampered = parts.join(".");
        assert!(keys.verify(&tampered).is_err());

        for bad in ["", "not-a-token", "a.b", "a.b.c", &token[..token.len() - 2]] {
            assert!(keys.verify(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn expired_tokens_are_rejected() {
        let keys = SigningKeys::generate().expect("generate");
        let mut claims = claims(&keys);
        // Well past the verification leeway.
        claims.exp = claims.iat - 3600;
        let token = keys.sign(&claims).expect("sign");
        let err = keys.verify(&token).expect_err("expired token");
        assert_eq!(err.status(), 401, "{err}");
    }

    #[test]
    fn keys_survive_a_der_round_trip() {
        let keys = SigningKeys::generate().expect("generate");
        let restored = SigningKeys::from_der(keys.to_der()).expect("restore");

        assert_eq!(restored.kid(), keys.kid(), "the key id must be stable");

        // A token from one verifies with the other.
        let token = keys.sign(&claims(&keys)).expect("sign");
        assert!(restored.verify(&token).is_ok());
    }

    #[test]
    fn invalid_key_material_is_refused() {
        for der in [&b""[..], &[1, 2, 3][..], &[0u8; 32][..]] {
            assert!(
                SigningKeys::from_der(der).is_err(),
                "{der:?} should be refused"
            );
        }
    }

    #[test]
    fn the_jwk_publishes_only_public_material() {
        let keys = SigningKeys::generate().expect("generate");
        let jwk = keys.jwk();

        assert_eq!(jwk["kty"], "OKP");
        assert_eq!(jwk["crv"], "Ed25519");
        assert_eq!(jwk["alg"], "EdDSA");
        assert_eq!(jwk["kid"], keys.kid());

        // 32 bytes base64url, no padding, and nothing resembling the private key.
        let x = jwk["x"].as_str().expect("x");
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(x)
                .unwrap()
                .len(),
            32
        );
        let serialized = jwk.to_string();
        assert!(
            !serialized.contains(&hex(keys.to_der())),
            "the private key leaked into the JWK"
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn claims_carry_the_subject_and_expiry() {
        let keys = SigningKeys::generate().expect("generate");
        let claims = keys
            .claims_for(Subject::Superuser, "su1", "", "", "tk")
            .unwrap();
        assert!(claims.typ.is_superuser());
        assert_eq!(claims.sub, "su1");
        assert_eq!(claims.exp - claims.iat, ACCESS_TOKEN_TTL.as_secs() as i64);
    }
}
