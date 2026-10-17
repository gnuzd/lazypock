//! The authentication service: signing keys plus the login and refresh flows.
//!
//! Access tokens are short-lived and verified from the public key alone, with no
//! database round trip. Long-lived state lives in a refresh session, which *is*
//! checked on every refresh — that is where revocation takes effect.

use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwapOption;
use base64::Engine;
use lazypock_core::error::{Error, Result};
use lazypock_core::secret::SecretCipher;
use lazypock_db::collections::Collection;
use lazypock_db::ddl::quote_ident;
use lazypock_db::pool::{db_error, Db};
use lazypock_db::runner::QueryRunner;
use serde_json::{json, Value};

use crate::password;
use crate::sessions;
use crate::superuser;
use crate::token::{Claims, SigningKeys, Subject, ACCESS_TOKEN_TTL};

/// The `_settings` key the signing key is stored under.
const SIGNING_KEY_SETTING: &str = "auth.signing_key";

/// What a successful authentication produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthOutcome {
    /// Short-lived bearer token.
    pub access_token: String,
    /// Opaque refresh token.
    pub refresh_token: String,
    /// Access-token lifetime in seconds.
    pub expires_in: i64,
    /// What was authenticated.
    pub subject: Subject,
    /// The account's id.
    pub user_id: String,
    /// Collection id; empty for a superuser.
    pub collection_id: String,
    /// Collection name; empty for a superuser.
    pub collection_name: String,
}

/// The account a session belongs to.
struct Account {
    subject: Subject,
    id: String,
    email: String,
    collection_id: String,
    collection_name: String,
    token_key: String,
}

/// Authentication: signing keys, login and refresh.
pub struct AuthService {
    db: Db,
    keys: ArcSwapOption<SigningKeys>,
}

impl std::fmt::Debug for AuthService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthService")
            .field("keys_loaded", &self.keys.load().is_some())
            .finish_non_exhaustive()
    }
}

impl AuthService {
    /// Build the service. [`AuthService::load`] must run before it is used.
    pub fn new(db: Db) -> Self {
        Self {
            db,
            keys: ArcSwapOption::empty(),
        }
    }

    /// Load the signing key.
    ///
    /// `LAZYPOCK_SIGNING_KEY` wins when it is set, and is **never written to the
    /// database**. That is the point of the override: a signing key in `_settings`
    /// is a signing key in every dump, replica and SQL console, and it mints every
    /// token the installation issues. Supplying it from the environment keeps it out
    /// of all of them.
    ///
    /// Without the override the key is read from `_settings`, and generated there on
    /// first run — convenient for a single node, and the reason it is still the
    /// fallback rather than being removed: a restart must not silently invalidate
    /// every session when no key was configured.
    ///
    /// The database copy is **encrypted with `LAZYPOCK_SECRET_KEY`** when that is set,
    /// because it has the same problem as a stored S3 secret and a worse consequence: a
    /// read-only copy of the database plus a signing key mints a token for any account,
    /// superusers included. A plaintext key from an earlier version is read and rewritten
    /// encrypted on the spot; a key that cannot be decrypted is a boot failure that names
    /// the variable, never a quiet regeneration — that would sign everybody out and look
    /// like a bug in the tokens.
    pub async fn load(&self) -> Result<()> {
        if let Some(keys) = signing_key_from_env()? {
            tracing::info!(
                kid = keys.kid(),
                "using the signing key from LAZYPOCK_SIGNING_KEY; the database copy is not used"
            );
            self.keys.store(Some(Arc::new(keys)));
            return Ok(());
        }

        // Read here rather than in `new`: a malformed key is a boot error, and this is
        // the only path that needs it.
        let cipher = SecretCipher::from_env()?;
        let client = self.db.client().await?;
        let keys = match read_key(&client, cipher.as_ref()).await? {
            Some(stored) => {
                let keys = SigningKeys::from_der(&stored.der)?;
                if !stored.encrypted {
                    if let Some(cipher) = cipher.as_ref() {
                        // Back-compat is read-and-upgrade, not read-and-leave: the next
                        // dump should not carry the key in the clear.
                        write_key(&client, &keys, Some(cipher)).await?;
                        tracing::info!(
                            kid = keys.kid(),
                            "the stored signing key was plaintext; it is now encrypted with \
                             LAZYPOCK_SECRET_KEY"
                        );
                    } else {
                        tracing::warn!(
                            kid = keys.kid(),
                            "the signing key is stored as plaintext in _settings, so every dump \
                             and replica of this database mints tokens for any account. Set \
                             LAZYPOCK_SECRET_KEY and restart to encrypt it at rest."
                        );
                    }
                }
                keys
            }
            None => {
                let keys = SigningKeys::generate()?;
                write_key(&client, &keys, cipher.as_ref()).await?;
                tracing::info!(kid = keys.kid(), "generated an access-token signing key");
                if cipher.is_none() {
                    tracing::warn!(
                        kid = keys.kid(),
                        "the generated signing key is stored as plaintext in _settings. Set \
                         LAZYPOCK_SECRET_KEY to encrypt it at rest."
                    );
                }
                keys
            }
        };
        tracing::debug!(kid = keys.kid(), "access-token signing key ready");
        self.keys.store(Some(Arc::new(keys)));
        Ok(())
    }

    /// The current signing keys. Fails loudly if [`AuthService::load`] has not
    /// run, rather than silently issuing unsigned tokens.
    pub fn keys(&self) -> Result<Arc<SigningKeys>> {
        self.keys
            .load_full()
            .ok_or_else(|| Error::config("the signing key has not been loaded"))
    }

    /// The public half of the signing key, as a JWKS document.
    pub fn jwks(&self) -> Result<Value> {
        Ok(json!({ "keys": [self.keys()?.jwk()] }))
    }

    /// Verify an access token.
    pub fn verify(&self, token: &str) -> Result<Claims> {
        self.keys()?.verify(token)
    }

    /// Authenticate a superuser by email and password.
    pub async fn authenticate_superuser(&self, email: &str, pass: &str) -> Result<AuthOutcome> {
        let client = self.db.client().await?;
        let found = superuser::find_by_email(&client, email).await?;

        // Verify against *something* even when the account does not exist, so the
        // response time does not reveal whether an email is registered.
        let (stored, account) = match found {
            Some(account) => (account.password_hash.clone(), Some(account)),
            None => (dummy_hash().to_string(), None),
        };
        let verified = password::verify(pass, &stored);

        let Some(account) = account.filter(|_| verified) else {
            return Err(Error::unauthorized("invalid email or password"));
        };

        self.issue(
            Subject::Superuser,
            &account.id,
            &account.email,
            "",
            "",
            &account.token_key,
        )
        .await
    }

    /// Authenticate a record in an auth collection.
    pub async fn authenticate_record(
        &self,
        collection: &Collection,
        email: &str,
        pass: &str,
    ) -> Result<AuthOutcome> {
        if !collection.collection_type.is_auth() {
            return Err(Error::invalid(format!(
                "{:?} is not an auth collection",
                collection.name
            )));
        }

        let client = self.db.client().await?;
        let table = quote_ident(&collection.name);
        let row = client
            .run_query_opt(
                &format!(
                    "select \"id\", \"email\", \"password_hash\", \"token_key\" from {table} \
                     where lower(\"email\") = lower($1::text)"
                ),
                &[&email.trim().to_string()],
            )
            .await
            .map_err(db_error)?;

        let (stored, account) = match &row {
            Some(row) => (
                row.get::<_, Option<String>>("password_hash")
                    .unwrap_or_else(|| dummy_hash().to_string()),
                Some(row),
            ),
            None => (dummy_hash().to_string(), None),
        };
        let verified = password::verify(pass, &stored);

        let Some(row) = account.filter(|_| verified) else {
            return Err(Error::unauthorized("invalid email or password"));
        };

        let id: String = row.get("id");
        let email: String = row.get::<_, Option<String>>("email").unwrap_or_default();
        let token_key: String = row
            .get::<_, Option<String>>("token_key")
            .unwrap_or_default();
        self.issue(
            Subject::Auth,
            &id,
            &email,
            &collection.id,
            &collection.name,
            &token_key,
        )
        .await
    }

    /// Issue a token pair for a record already authenticated by another mechanism.
    ///
    /// An OAuth2 sign-in (and later an OTP) has verified the identity without a
    /// password; this reads the account's email and token key exactly as a
    /// password sign-in does, so the token it mints is indistinguishable from one
    /// issued after a password check.
    pub async fn issue_for_record(
        &self,
        collection: &Collection,
        record_id: &str,
    ) -> Result<AuthOutcome> {
        let client = self.db.client().await?;
        let table = quote_ident(&collection.name);
        let row = client
            .run_query_opt(
                &format!("select \"email\", \"token_key\" from {table} where \"id\" = $1::text"),
                &[&record_id.to_string()],
            )
            .await
            .map_err(db_error)?
            .ok_or_else(|| Error::unauthorized("the account no longer exists"))?;

        let email: String = row.get::<_, Option<String>>("email").unwrap_or_default();
        let token_key: String = row
            .get::<_, Option<String>>("token_key")
            .unwrap_or_default();

        self.issue(
            Subject::Auth,
            record_id,
            &email,
            &collection.id,
            &collection.name,
            &token_key,
        )
        .await
    }

    /// Exchange a refresh token for a new token pair.
    ///
    /// The account is re-read, so a deleted record or a removed collection ends the
    /// session here rather than at access-token expiry. The account's `token_key` is
    /// carried into the new access token but is **not** checked against anything: a
    /// password change ends sessions by revoking them (`sessions::revoke_for_user`),
    /// not by rotating the key.
    pub async fn refresh(
        &self,
        refresh_token: &str,
        collections: &[Collection],
        expected_collection: Option<&str>,
    ) -> Result<AuthOutcome> {
        let client = self.db.client().await?;

        // The collection check runs **before** the rotation, not after it. Rotation is
        // destructive, so checking afterwards consumed the token and then threw away its
        // replacement: a refresh sent to the wrong collection's endpoint — a stale SDK, a
        // hand-built URL — logged the user out with no way back. `peek` reads the session
        // without retiring it.
        if let Some(expected) = expected_collection {
            let session = sessions::peek(&client, refresh_token)
                .await?
                .ok_or_else(|| Error::unauthorized("the refresh token is not recognised"))?;
            let matches = if session.collection_id.is_empty() {
                // A superuser session has no collection, so it belongs to the
                // `_superusers` path and to no other.
                expected == superuser::COLLECTION
            } else {
                collections.iter().any(|collection| {
                    collection.id == session.collection_id && collection.name == expected
                })
            };
            if !matches {
                return Err(Error::unauthorized(
                    "the refresh token belongs to a different collection",
                ));
            }
        }

        // Rotation already produced the next refresh token, in the same family,
        // and it must be the one handed back. Issuing a fresh session here would
        // put every refresh in its own family and quietly disable reuse
        // detection, so that replaying an old token would never revoke the chain.
        let (refresh_token, session) = sessions::rotate(&client, refresh_token, None, None).await?;
        let account = self.account_for(&client, &session, collections).await?;

        Ok(AuthOutcome {
            access_token: self.sign(
                account.subject,
                &account.id,
                &account.email,
                &account.collection_id,
                &account.collection_name,
                &account.token_key,
            )?,
            refresh_token,
            expires_in: ACCESS_TOKEN_TTL.as_secs() as i64,
            subject: account.subject,
            user_id: account.id,
            collection_id: account.collection_id,
            collection_name: account.collection_name,
        })
    }

    /// Re-read the account a session belongs to.
    ///
    /// Done on every refresh, so a deleted record or a removed collection ends the
    /// session here rather than at access-token expiry.
    async fn account_for(
        &self,
        client: &lazypock_db::deadpool_postgres::Client,
        session: &sessions::Session,
        collections: &[Collection],
    ) -> Result<Account> {
        if session.collection_id.is_empty() {
            let account = superuser::find_by_id(client, &session.user_id)
                .await?
                .ok_or_else(|| Error::unauthorized("the account no longer exists"))?;
            return Ok(Account {
                subject: Subject::Superuser,
                id: account.id,
                email: account.email,
                collection_id: String::new(),
                collection_name: String::new(),
                token_key: account.token_key,
            });
        }

        let collection = collections
            .iter()
            .find(|candidate| candidate.id == session.collection_id)
            .ok_or_else(|| Error::unauthorized("the collection no longer exists"))?;
        let table = quote_ident(&collection.name);
        let row = client
            .run_query_opt(
                &format!("select \"email\", \"token_key\" from {table} where \"id\" = $1::text"),
                &[&session.user_id],
            )
            .await
            .map_err(db_error)?
            .ok_or_else(|| Error::unauthorized("the account no longer exists"))?;

        Ok(Account {
            subject: Subject::Auth,
            id: session.user_id.clone(),
            email: row.get::<_, Option<String>>("email").unwrap_or_default(),
            collection_id: collection.id.clone(),
            collection_name: collection.name.clone(),
            token_key: row
                .get::<_, Option<String>>("token_key")
                .unwrap_or_default(),
        })
    }

    /// Sign an access token for an account.
    pub fn sign(
        &self,
        subject: Subject,
        id: &str,
        email: &str,
        collection_id: &str,
        collection_name: &str,
        token_key: &str,
    ) -> Result<String> {
        let keys = self.keys()?;
        let claims = keys.claims_for(
            subject,
            id,
            email,
            collection_id,
            collection_name,
            token_key,
        )?;
        keys.sign(&claims)
    }

    async fn issue(
        &self,
        subject: Subject,
        id: &str,
        email: &str,
        collection_id: &str,
        collection_name: &str,
        token_key: &str,
    ) -> Result<AuthOutcome> {
        let client = self.db.client().await?;
        let (refresh_token, _session) =
            sessions::issue(&client, collection_id, id, None, None).await?;

        Ok(AuthOutcome {
            access_token: self.sign(
                subject,
                id,
                email,
                collection_id,
                collection_name,
                token_key,
            )?,
            refresh_token,
            expires_in: ACCESS_TOKEN_TTL.as_secs() as i64,
            subject,
            user_id: id.to_string(),
            collection_id: collection_id.to_string(),
            collection_name: collection_name.to_string(),
        })
    }
}

/// A valid hash to compare against when an account does not exist, so a missing
/// account and a wrong password cost the same.
fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        password::hash("lazypock-placeholder-password").expect("the placeholder hash must be valid")
    })
}

/// The signing key from `LAZYPOCK_SIGNING_KEY`, base64 of the raw key.
///
/// The same encoding the database copy uses, so a key can be moved between the two
/// without a conversion step, and an operator can copy one out of a running
/// installation with a single query.
fn signing_key_from_env() -> Result<Option<SigningKeys>> {
    let Ok(raw) = std::env::var("LAZYPOCK_SIGNING_KEY") else {
        return Ok(None);
    };
    let raw = raw.trim();
    if raw.is_empty() {
        // A set-but-empty variable is almost always a deploy templating mistake, and
        // silently falling back to the database would hide it until the tokens
        // quietly changed signer.
        return Err(Error::config(
            "LAZYPOCK_SIGNING_KEY is set but empty; unset it to use the database copy, \
             or give it a base64 key",
        ));
    }

    let der = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| Error::config("LAZYPOCK_SIGNING_KEY is not valid base64"))?;
    Ok(Some(SigningKeys::from_der(&der)?))
}

/// A signing key as `_settings` holds it.
struct StoredKey {
    /// The DER bytes the tokens are signed with.
    der: Vec<u8>,
    /// Whether the stored form was ciphertext rather than base64.
    encrypted: bool,
}

async fn read_key(
    runner: &dyn QueryRunner,
    cipher: Option<&SecretCipher>,
) -> Result<Option<StoredKey>> {
    let row = runner
        .run_query_opt(
            "select value from _settings where key = $1::text",
            &[&SIGNING_KEY_SETTING.to_string()],
        )
        .await
        .map_err(db_error)?;

    let Some(row) = row else {
        return Ok(None);
    };
    let value: Value = row.get("value");
    let Some(encoded) = value.get("der").and_then(Value::as_str) else {
        return Err(Error::config(
            "the stored signing key has no `der`; remove the auth.signing_key setting to regenerate",
        ));
    };

    // An encrypted value without the key is a hard stop, not a reason to generate a new
    // one: regenerating would sign everybody out, and the operator would be looking at a
    // mass logout instead of at the variable that is missing.
    let encrypted = SecretCipher::is_encrypted(encoded);
    let encoded = if encrypted {
        let cipher = cipher.ok_or_else(|| {
            Error::config(format!(
                "the stored signing key is encrypted but {} is not set, so it cannot be read; \
                 set it to the key it was saved with (`openssl rand -hex 32` for a new one, \
                 which signs everybody out)",
                lazypock_core::secret::KEY_VARIABLE
            ))
        })?;
        cipher.decrypt(SIGNING_KEY_SETTING, encoded)?
    } else {
        encoded.to_string()
    };

    let der = base64::engine::general_purpose::STANDARD
        .decode(&encoded)
        .map_err(|_| Error::config("the stored signing key is not valid base64"))?;
    Ok(Some(StoredKey { der, encrypted }))
}

async fn write_key(
    runner: &dyn QueryRunner,
    keys: &SigningKeys,
    cipher: Option<&SecretCipher>,
) -> Result<()> {
    let der = base64::engine::general_purpose::STANDARD.encode(keys.to_der());
    let der = match cipher {
        Some(cipher) => cipher.encrypt(SIGNING_KEY_SETTING, &der)?,
        None => der,
    };
    let value = json!({
        "kid": keys.kid(),
        "der": der,
    });
    runner
        .run_execute(
            "insert into _settings (key, value) values ($1::text, $2::text::jsonb) \
             on conflict (key) do update set value = excluded.value, updated = now()",
            &[&SIGNING_KEY_SETTING.to_string(), &value.to_string()],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}
