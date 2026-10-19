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
    /// The account's email, so rules can compare against `@request.auth.email`.
    pub email: String,
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

/// Length of an Ed25519 private seed, in bytes.
pub const ED25519_SEED_LEN: usize = 32;

/// The fixed PKCS#8 v2 header of an Ed25519 private key: a `OneAsymmetricKey` with the
/// `id-Ed25519` algorithm identifier and the private-key OCTET STRING header. The
/// 32-byte seed follows it, then [`ED25519_PKCS8_PUBLIC_TAG`] and the 32-byte public key.
///
/// This byte-for-byte matches what `ring`'s `generate_pkcs8` writes (including the
/// legacy `[1]` public-key tag it accepts), so a key derived from a seed and a generated
/// one are interchangeable. Kept as a constant rather than pulled in from an ASN.1 crate
/// for one fixed encoding; [`SigningKeys::from_der`] parses the result with the same
/// parser a stored key goes through, so a wrong constant cannot go unnoticed.
const ED25519_PKCS8_HEADER: [u8; 16] = [
    0x30, 0x51, // SEQUENCE, 81 bytes
    0x02, 0x01, 0x01, // INTEGER 1 (v2, public key present)
    0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, // AlgorithmIdentifier: 1.3.101.112
    0x04, 0x22, // OCTET STRING, 34 bytes
    0x04, 0x20, // inner OCTET STRING, 32 bytes: the seed
];

/// The context tag + length that introduce the public key in a PKCS#8 v2 document
/// (legacy `[1]` form, which is what `ring` emits and accepts).
const ED25519_PKCS8_PUBLIC_TAG: [u8; 3] = [0x81, 0x21, 0x00];

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

    /// Build from a 32-byte Ed25519 seed — a raw private key.
    ///
    /// This is the shape `openssl rand -base64 32` produces, which is what the
    /// documentation and `.env.example` tell an operator to use for
    /// `LAZYPOCK_SIGNING_KEY`. Ed25519 private keys are seeds, so the PKCS#8 document
    /// the rest of this type works with is derived here from the seed and its public
    /// half, rather than required of the caller.
    ///
    /// [`SigningKeys::from_der`] remains the path for a key copied out of an
    /// installation or a backup, where the document already exists.
    pub fn from_seed(seed: &[u8]) -> Result<Self> {
        if seed.len() != ED25519_SEED_LEN {
            return Err(Error::config(format!(
                "an Ed25519 signing key is {ED25519_SEED_LEN} bytes, got {}",
                seed.len()
            )));
        }
        let pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|err| Error::config(format!("the signing key seed is unusable: {err}")))?;

        let mut der = Vec::with_capacity(
            ED25519_PKCS8_HEADER.len()
                + ED25519_SEED_LEN
                + ED25519_PKCS8_PUBLIC_TAG.len()
                + ED25519_SEED_LEN,
        );
        der.extend_from_slice(&ED25519_PKCS8_HEADER);
        der.extend_from_slice(seed);
        der.extend_from_slice(&ED25519_PKCS8_PUBLIC_TAG);
        der.extend_from_slice(pair.public_key().as_ref());
        Self::from_der(&der)
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
        email: &str,
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
            email: email.to_string(),
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
        keys.claims_for(
            Subject::Auth,
            "user1",
            "a@example.com",
            "col1",
            "users",
            "tk1",
        )
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
    fn a_seed_and_its_pkcs8_document_are_the_same_key() {
        // The documented `openssl rand -base64 32` supplies the 32-byte seed; a stored
        // key is its PKCS#8 document. They must produce the same key, or a value moved
        // between the environment and `_settings` would silently sign with a different
        // key.
        let keys = SigningKeys::generate().expect("generate");
        let der = keys.to_der().to_vec();
        assert_eq!(
            der.len(),
            ED25519_PKCS8_HEADER.len()
                + ED25519_SEED_LEN
                + ED25519_PKCS8_PUBLIC_TAG.len()
                + ED25519_SEED_LEN
        );
        assert_eq!(
            &der[..ED25519_PKCS8_HEADER.len()],
            &ED25519_PKCS8_HEADER,
            "the fixed header does not describe the generated document"
        );

        let from_seed = SigningKeys::from_seed(
            &der[ED25519_PKCS8_HEADER.len()..ED25519_PKCS8_HEADER.len() + ED25519_SEED_LEN],
        )
        .expect("seed");
        assert_eq!(from_seed.kid(), keys.kid());
        assert_eq!(
            from_seed.to_der(),
            keys.to_der(),
            "a seed must produce exactly the document a generated key produces"
        );
        // A token from one verifies under the other.
        let token = from_seed.sign(&claims(&from_seed)).expect("sign");
        keys.verify(&token).expect("verify");
    }

    #[test]
    fn a_seed_of_the_wrong_length_is_refused() {
        for bad in [vec![], vec![7u8; 16], vec![7u8; 31], vec![7u8; 33]] {
            let err = SigningKeys::from_seed(&bad).expect_err("refused");
            assert!(err.message().contains("32 bytes"), "{err}");
        }
    }

    #[test]
    fn claims_carry_the_subject_and_expiry() {
        let keys = SigningKeys::generate().expect("generate");
        let claims = keys
            .claims_for(Subject::Superuser, "su1", "a@example.com", "", "", "tk")
            .unwrap();
        assert!(claims.typ.is_superuser());
        assert_eq!(claims.sub, "su1");
        assert_eq!(claims.email, "a@example.com");
        assert_eq!(claims.exp - claims.iat, ACCESS_TOKEN_TTL.as_secs() as i64);
    }
}
