//! `AuthService`: the login, refresh and verification flows.
//!
//! One instance per process (build it in `main`, share it in `AppState`). It owns
//! the signing key; everything else is stateless and reads the database.

use crate::password;
use crate::pg::q_opt;
use crate::sessions;
use crate::superuser;
use crate::token::{ACCESS_TOKEN_TTL, Claims, ED25519_SEED_LEN, SigningKeys, Subject};
use arc_swap::ArcSwapOption;
use base64::Engine;
use lazypock_core::secret::{KEY_VARIABLE, SecretCipher};
use lazypock_core::{Error, Result};
use lazypock_db::{Collection, Db, RequestContext, Schema};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};
use tokio_postgres::GenericClient;

const SIGNING_KEY_SETTING: &str = "auth.signing_key";
const SIGNING_KEY_VARIABLE: &str = "LAZYPOCK_SIGNING_KEY";
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// What the caller learned about the client, stored with the session.
#[derive(Debug, Clone, Default)]
pub struct ClientMeta {
    pub user_agent: Option<String>,
    pub ip: Option<String>,
}

/// Tokens and who they belong to.
#[derive(Clone)]
pub struct AuthOutcome {
    /// Short-lived bearer token (15 minutes).
    pub access_token: String,
    /// Opaque, single-use refresh token (30 days, rotated on every use).
    pub refresh_token: String,
    /// Access-token lifetime in seconds.
    pub expires_in: i64,
    pub subject: Subject,
    pub user_id: String,
    /// Empty for a superuser.
    pub collection_id: String,
    pub collection_name: String,
}

impl std::fmt::Debug for AuthOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Tokens are credentials: never let them reach a log line.
        f.debug_struct("AuthOutcome")
            .field("subject", &self.subject)
            .field("user_id", &self.user_id)
            .field("collection_name", &self.collection_name)
            .finish_non_exhaustive()
    }
}

pub struct AuthService {
    db: Db,
    cipher: Option<SecretCipher>,
    keys: ArcSwapOption<SigningKeys>,
}

impl std::fmt::Debug for AuthService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthService")
            .field(
                "kid",
                &self.keys.load().as_ref().map(|k| k.kid().to_string()),
            )
            .finish_non_exhaustive()
    }
}

/// The identity a token or session resolves to.
struct Account {
    subject: Subject,
    id: String,
    email: String,
    collection_id: String,
    collection_name: String,
    token_key: String,
}

impl AuthService {
    /// `cipher` encrypts the signing key at rest: pass `SecretCipher::from_env()?`.
    /// Call [`AuthService::load`] before serving.
    pub fn new(db: Db, cipher: Option<SecretCipher>) -> Self {
        AuthService {
            db,
            cipher,
            keys: ArcSwapOption::empty(),
        }
    }

    // ---- signing key -------------------------------------------------------

    /// Load (or create) the signing key. Order: `LAZYPOCK_SIGNING_KEY`, then the
    /// copy in `_settings`, else a fresh key stored there. Safe to run on several
    /// nodes at once: the first insert wins and everyone adopts it.
    pub async fn load(&self) -> Result<()> {
        dummy_hash(); // pay the one-time cost now, not on the first failed login

        if let Some(keys) = signing_key_from_env()? {
            tracing::info!(
                kid = keys.kid(),
                "using the signing key from {SIGNING_KEY_VARIABLE}; the database copy is not used"
            );
            self.keys.store(Some(Arc::new(keys)));
            return Ok(());
        }

        let cipher = self.cipher.as_ref();
        let conn = self.db.conn().await?;
        let client = &**conn;
        let keys = match read_key(client, cipher).await? {
            Some(stored) => {
                let keys = SigningKeys::from_der(&stored.der)?;
                if !stored.encrypted {
                    if cipher.is_some() {
                        replace_key(client, &keys, cipher).await?;
                        tracing::info!(
                            kid = keys.kid(),
                            "the stored signing key was plaintext; it is now encrypted with {KEY_VARIABLE}"
                        );
                    } else {
                        tracing::warn!(
                            kid = keys.kid(),
                            "the signing key is stored as plaintext in _settings, so every dump and replica of this database can mint tokens for any account. Set {KEY_VARIABLE} and restart to encrypt it at rest."
                        );
                    }
                }
                keys
            }
            None => {
                let fresh = SigningKeys::generate()?;
                if insert_key_if_absent(client, &fresh, cipher).await? {
                    tracing::info!(kid = fresh.kid(), "generated an access-token signing key");
                    if cipher.is_none() {
                        tracing::warn!(
                            "the generated signing key is stored as plaintext in _settings. Set {KEY_VARIABLE} to encrypt it at rest."
                        );
                    }
                }
                // Another node may have inserted first: whatever is stored is the key.
                let stored = read_key(client, cipher).await?.ok_or_else(|| {
                    Error::Internal("the signing key vanished right after it was stored".into())
                })?;
                SigningKeys::from_der(&stored.der)?
            }
        };
        tracing::debug!(kid = keys.kid(), "access-token signing key ready");
        self.keys.store(Some(Arc::new(keys)));
        Ok(())
    }

    pub fn keys(&self) -> Result<Arc<SigningKeys>> {
        self.keys.load_full().ok_or_else(|| {
            Error::Internal(
                "the signing key is not loaded; call AuthService::load() at startup".into(),
            )
        })
    }

    /// Public keys in JWKS form.
    pub fn jwks(&self) -> Result<Value> {
        Ok(json!({ "keys": [self.keys()?.jwk()] }))
    }

    // ---- verification ------------------------------------------------------

    /// Check signature and expiry. No database access.
    pub fn verify(&self, token: &str) -> Result<Claims> {
        self.keys()?.verify(token)
    }

    /// The request context for a verified token, **without** touching the
    /// database. Cheap, but it cannot notice a token revoked by a password change
    /// (its `tkey` is not re-checked) and offers no `@request.auth.<field>` claims.
    pub fn context_from_claims(&self, claims: &Claims) -> RequestContext {
        if claims.typ.is_superuser() {
            return RequestContext::owner();
        }
        RequestContext::authenticated(&claims.sub, &claims.email, &claims.col, &claims.name)
    }

    /// The request context for a verified token, **with** one query: confirms the
    /// account still exists and the token key still matches (so a password change
    /// or [`AuthService::revoke_all`]-style rotation takes effect immediately) and
    /// loads the record's columns, minus credentials, for `@request.auth.<field>`.
    pub async fn request_context(
        &self,
        claims: &Claims,
        schema: &Schema,
    ) -> Result<RequestContext> {
        let conn = self.db.conn().await?;
        let client = &**conn;
        if claims.typ.is_superuser() {
            let su = superuser::find_by_id(client, &claims.sub)
                .await?
                .ok_or_else(|| Error::Unauthorized("the account no longer exists".into()))?;
            check_token_key(&su.token_key, &claims.tkey)?;
            return Ok(RequestContext::owner());
        }

        let collection = schema
            .get_by_id(&claims.col)
            .ok_or_else(|| Error::Unauthorized("the collection no longer exists".into()))?;
        let table = collection.name.quoted();
        let row = q_opt(
            client,
            &format!("select \"token_key\", (to_jsonb(t) - 'password_hash' - 'token_key')::text from {table} t where t.\"id\" = $1::text"),
            &[&claims.sub],
        )
        .await?
        .ok_or_else(|| Error::Unauthorized("the account no longer exists".into()))?;
        check_token_key(
            &row.get::<_, Option<String>>(0).unwrap_or_default(),
            &claims.tkey,
        )?;
        let record: String = row.get(1);
        Ok(RequestContext::authenticated(
            &claims.sub,
            &claims.email,
            &collection.id,
            collection.name.as_str(),
        )
        .with_auth_claims(record))
    }

    // ---- login -------------------------------------------------------------

    pub async fn authenticate_superuser(
        &self,
        email: &str,
        pass: &str,
        meta: &ClientMeta,
    ) -> Result<AuthOutcome> {
        let found = {
            let conn = self.db.conn().await?;
            superuser::find_by_email(&**conn, email).await?
        };
        // Always run one Argon2 verification, hit or miss, so response time does not
        // reveal whether the address exists.
        let stored = found
            .as_ref()
            .map_or_else(|| dummy_hash().to_string(), |a| a.password_hash.clone());
        let verified = verify_blocking(pass, stored).await;
        let Some(account) = found.filter(|_| verified) else {
            return Err(Error::Unauthorized("invalid email or password".into()));
        };
        self.issue(
            Subject::Superuser,
            &account.id,
            &account.email,
            "",
            "",
            &account.token_key,
            meta,
        )
        .await
    }

    /// Log in a record of an auth collection by email + password.
    pub async fn authenticate_record(
        &self,
        collection: &Collection,
        email: &str,
        pass: &str,
        meta: &ClientMeta,
    ) -> Result<AuthOutcome> {
        ensure_auth(collection)?;
        let table = collection.name.quoted();
        let row = {
            let conn = self.db.conn().await?;
            q_opt(
                &**conn,
                &format!("select \"id\", \"email\", \"password_hash\", \"token_key\" from {table} where lower(\"email\") = lower($1::text)"),
                &[email.trim()],
            )
            .await?
        };
        let stored = row
            .as_ref()
            .and_then(|r| r.get::<_, Option<String>>(2))
            .unwrap_or_else(|| dummy_hash().to_string());
        let verified = verify_blocking(pass, stored).await;
        let Some(row) = row.filter(|_| verified) else {
            return Err(Error::Unauthorized("invalid email or password".into()));
        };
        let (id, email, token_key): (String, Option<String>, Option<String>) =
            (row.get(0), row.get(1), row.get(3));
        self.issue(
            Subject::Auth,
            &id,
            &email.unwrap_or_default(),
            &collection.id,
            collection.name.as_str(),
            &token_key.unwrap_or_default(),
            meta,
        )
        .await
    }

    /// Start a session for a record without a password: after OAuth, OTP or email
    /// verification has proven who it is.
    pub async fn issue_for_record(
        &self,
        collection: &Collection,
        record_id: &str,
        meta: &ClientMeta,
    ) -> Result<AuthOutcome> {
        ensure_auth(collection)?;
        let table = collection.name.quoted();
        let row = {
            let conn = self.db.conn().await?;
            q_opt(
                &**conn,
                &format!("select \"email\", \"token_key\" from {table} where \"id\" = $1::text"),
                &[record_id],
            )
            .await?
        }
        .ok_or_else(|| Error::Unauthorized("the account no longer exists".into()))?;
        let (email, token_key): (Option<String>, Option<String>) = (row.get(0), row.get(1));
        self.issue(
            Subject::Auth,
            record_id,
            &email.unwrap_or_default(),
            &collection.id,
            collection.name.as_str(),
            &token_key.unwrap_or_default(),
            meta,
        )
        .await
    }

    // ---- refresh / logout --------------------------------------------------

    /// Exchange a refresh token for a new pair. The old refresh token stops working;
    /// presenting it again revokes the whole session family.
    ///
    /// `expected_collection` pins the token to a collection (`Some("users")`, or
    /// `Some("_superusers")`) so a token for one cannot be used on another's endpoint.
    pub async fn refresh(
        &self,
        refresh_token: &str,
        schema: &Schema,
        expected_collection: Option<&str>,
        meta: &ClientMeta,
    ) -> Result<AuthOutcome> {
        let conn = self.db.conn().await?;
        let client = &**conn;
        if let Some(expected) = expected_collection {
            let session = sessions::peek(client, refresh_token)
                .await?
                .ok_or_else(|| Error::Unauthorized("the refresh token is not recognised".into()))?;
            let matches = if session.collection_id.is_empty() {
                expected == superuser::COLLECTION
            } else {
                schema
                    .get_by_id(&session.collection_id)
                    .is_some_and(|c| c.name.as_str() == expected)
            };
            if !matches {
                return Err(Error::Unauthorized(
                    "the refresh token belongs to a different collection".into(),
                ));
            }
        }
        let (refresh_token, session) = sessions::rotate(
            client,
            refresh_token,
            meta.user_agent.as_deref(),
            meta.ip.as_deref(),
        )
        .await?;
        let a = self.account_for(client, &session, schema).await?;
        Ok(AuthOutcome {
            access_token: self.sign(
                a.subject,
                &a.id,
                &a.email,
                &a.collection_id,
                &a.collection_name,
                &a.token_key,
            )?,
            refresh_token,
            expires_in: ACCESS_TOKEN_TTL.as_secs() as i64,
            subject: a.subject,
            user_id: a.id,
            collection_id: a.collection_id,
            collection_name: a.collection_name,
        })
    }

    /// End one login (the refresh token's whole family). Unknown tokens are fine.
    pub async fn logout(&self, refresh_token: &str) -> Result<()> {
        let conn = self.db.conn().await?;
        if let Some(session) = sessions::peek(&**conn, refresh_token).await? {
            sessions::revoke_family(&**conn, &session.family_id).await?;
        }
        Ok(())
    }

    /// Sign an account out everywhere (refresh tokens only; to also cut off
    /// outstanding access tokens, rotate the account's `token_key`).
    pub async fn revoke_all(&self, collection_id: &str, user_id: &str) -> Result<u64> {
        let conn = self.db.conn().await?;
        sessions::revoke_for_user(&**conn, collection_id, user_id).await
    }

    // ---- records of auth collections ---------------------------------------

    /// Turn the password fields of a create body into stored credentials.
    /// See [`crate::record::prepare_create`].
    pub async fn prepare_record_create(&self, body: &Value) -> Result<crate::record::Prepared> {
        crate::record::prepare_create(body).await
    }

    /// Turn the password fields of an update body into stored credentials. When
    /// `require_old` (everyone but a superuser) a password change must carry the
    /// current password as `oldPassword`.
    pub async fn prepare_record_update(
        &self,
        collection: &Collection,
        id: &str,
        body: &Value,
        require_old: bool,
    ) -> Result<crate::record::Prepared> {
        ensure_auth(collection)?;
        let wants_change = body.get("password").is_some_and(|p| !p.is_null());
        let current = if wants_change && require_old {
            let table = collection.name.quoted();
            let conn = self.db.conn().await?;
            q_opt(
                &**conn,
                &format!("select \"password_hash\" from {table} where \"id\" = $1::text"),
                &[id],
            )
            .await?
            .and_then(|r| r.get::<_, Option<String>>(0))
        } else {
            None
        };
        crate::record::prepare_update(current.as_deref(), body, require_old).await
    }

    // ---- bootstrap ---------------------------------------------------------

    /// Create the first superuser if there is none. Returns whether it created one.
    /// Safe to call from every node at boot.
    pub async fn ensure_superuser(&self, email: &str, password: &str) -> Result<bool> {
        let conn = self.db.conn().await?;
        if superuser::count(&**conn).await? > 0 {
            return Ok(false);
        }
        match superuser::create(&**conn, email, password).await {
            Ok(_) => Ok(true),
            Err(Error::Conflict(_)) => Ok(false), // another node won the race
            Err(e) => Err(e),
        }
    }

    // ---- internals ---------------------------------------------------------

    fn sign(
        &self,
        subject: Subject,
        id: &str,
        email: &str,
        collection_id: &str,
        collection_name: &str,
        token_key: &str,
    ) -> Result<String> {
        let keys = self.keys()?;
        keys.sign(&keys.claims_for(
            subject,
            id,
            email,
            collection_id,
            collection_name,
            token_key,
        )?)
    }

    #[allow(clippy::too_many_arguments)]
    async fn issue(
        &self,
        subject: Subject,
        id: &str,
        email: &str,
        collection_id: &str,
        collection_name: &str,
        token_key: &str,
        meta: &ClientMeta,
    ) -> Result<AuthOutcome> {
        let conn = self.db.conn().await?;
        let (refresh_token, _) = sessions::issue(
            &**conn,
            collection_id,
            id,
            meta.user_agent.as_deref(),
            meta.ip.as_deref(),
        )
        .await?;
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

    async fn account_for<C: GenericClient>(
        &self,
        client: &C,
        session: &sessions::Session,
        schema: &Schema,
    ) -> Result<Account> {
        if session.collection_id.is_empty() {
            let su = superuser::find_by_id(client, &session.user_id)
                .await?
                .ok_or_else(|| Error::Unauthorized("the account no longer exists".into()))?;
            return Ok(Account {
                subject: Subject::Superuser,
                id: su.id,
                email: su.email,
                collection_id: String::new(),
                collection_name: String::new(),
                token_key: su.token_key,
            });
        }
        let collection = schema
            .get_by_id(&session.collection_id)
            .ok_or_else(|| Error::Unauthorized("the collection no longer exists".into()))?;
        let table = collection.name.quoted();
        let row = q_opt(
            client,
            &format!("select \"email\", \"token_key\" from {table} where \"id\" = $1::text"),
            &[&session.user_id],
        )
        .await?
        .ok_or_else(|| Error::Unauthorized("the account no longer exists".into()))?;
        Ok(Account {
            subject: Subject::Auth,
            id: session.user_id.clone(),
            email: row.get::<_, Option<String>>(0).unwrap_or_default(),
            collection_id: collection.id.clone(),
            collection_name: collection.name.as_str().to_string(),
            token_key: row.get::<_, Option<String>>(1).unwrap_or_default(),
        })
    }
}

fn ensure_auth(collection: &Collection) -> Result<()> {
    if collection.ty.is_auth() {
        Ok(())
    } else {
        Err(Error::Validation(format!(
            "{:?} is not an auth collection",
            collection.name.as_str()
        )))
    }
}

fn check_token_key(stored: &str, claimed: &str) -> Result<()> {
    if stored == claimed {
        Ok(())
    } else {
        Err(Error::Unauthorized("the access token was revoked".into()))
    }
}

async fn verify_blocking(password: &str, stored: String) -> bool {
    password::verify_blocking(password.to_string(), stored).await
}

/// A hash of a throwaway password, verified against when no account matches.
pub(crate) fn dummy_hash_for_record() -> &'static str {
    dummy_hash()
}

fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        password::hash("lazypock timing pad, never a real password").unwrap_or_default()
    })
}

// ---- signing-key storage -------------------------------------------------------

fn signing_key_from_env() -> Result<Option<SigningKeys>> {
    let Ok(raw) = std::env::var(SIGNING_KEY_VARIABLE) else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Err(Error::Config(format!(
            "{SIGNING_KEY_VARIABLE} is set but empty; unset it to use the database copy, or give it base64 of 32 random bytes (`openssl rand -base64 32`)"
        )));
    }
    let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = B64.decode(&compact).map_err(|_| {
        Error::Config(format!("{SIGNING_KEY_VARIABLE} is not valid base64; give it base64 of 32 random bytes (`openssl rand -base64 32`)"))
    })?;
    if let Ok(keys) = SigningKeys::from_der(&bytes) {
        return Ok(Some(keys));
    }
    if bytes.len() == ED25519_SEED_LEN {
        return SigningKeys::from_seed(&bytes).map(Some);
    }
    Err(Error::Config(format!(
        "{SIGNING_KEY_VARIABLE} decodes to {} bytes, which is neither a 32-byte Ed25519 seed nor a PKCS#8 Ed25519 key (`openssl rand -base64 32` makes a valid one)",
        bytes.len()
    )))
}

struct StoredKey {
    der: Vec<u8>,
    encrypted: bool,
}

async fn read_key<C: GenericClient>(
    client: &C,
    cipher: Option<&SecretCipher>,
) -> Result<Option<StoredKey>> {
    let Some(row) = q_opt(
        client,
        "select value::text from _settings where key = $1::text",
        &[SIGNING_KEY_SETTING],
    )
    .await?
    else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&row.get::<_, String>(0))
        .map_err(|_| Error::Config("the stored signing key setting is not valid JSON".into()))?;
    let Some(encoded) = value.get("der").and_then(Value::as_str) else {
        return Err(Error::Config("the stored signing key has no `der`; remove the auth.signing_key setting to regenerate".into()));
    };
    let encrypted = SecretCipher::is_encrypted(encoded);
    let plain = if encrypted {
        let cipher = cipher.ok_or_else(|| {
            Error::Config(format!(
                "the stored signing key is encrypted but {KEY_VARIABLE} is not set, so it cannot be read; set it to the key it was saved with (a new one signs everybody out)"
            ))
        })?;
        cipher.decrypt(SIGNING_KEY_SETTING, encoded)?
    } else {
        encoded.to_string()
    };
    let der = B64
        .decode(&plain)
        .map_err(|_| Error::Config("the stored signing key is not valid base64".into()))?;
    Ok(Some(StoredKey { der, encrypted }))
}

fn key_value(keys: &SigningKeys, cipher: Option<&SecretCipher>) -> Result<String> {
    let der = B64.encode(keys.to_der());
    let der = match cipher {
        Some(c) => c.encrypt(SIGNING_KEY_SETTING, &der)?,
        None => der,
    };
    Ok(json!({ "kid": keys.kid(), "der": der }).to_string())
}

/// First writer wins; returns whether this call stored the key.
async fn insert_key_if_absent<C: GenericClient>(
    client: &C,
    keys: &SigningKeys,
    cipher: Option<&SecretCipher>,
) -> Result<bool> {
    let rows = crate::pg::q(
        client,
        "insert into _settings (key, value) values ($1::text, $2::text::jsonb) on conflict (key) do nothing returning key",
        &[SIGNING_KEY_SETTING, &key_value(keys, cipher)?],
    )
    .await?;
    Ok(!rows.is_empty())
}

async fn replace_key<C: GenericClient>(
    client: &C,
    keys: &SigningKeys,
    cipher: Option<&SecretCipher>,
) -> Result<()> {
    crate::pg::q(
        client,
        "update _settings set value = $2::text::jsonb, updated = now() where key = $1::text returning key",
        &[SIGNING_KEY_SETTING, &key_value(keys, cipher)?],
    )
    .await?;
    Ok(())
}
