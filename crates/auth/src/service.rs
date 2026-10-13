//! The authentication service: signing keys plus the login and refresh flows.
//!
//! Access tokens are short-lived and verified from the public key alone, with no
//! database round trip. Long-lived state lives in a refresh session, which *is*
//! checked on every refresh — that is where revocation takes effect.

use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwapOption;
use base64::Engine;
use lazypock_core::error::{Error, Result};
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

    /// Load the signing key, generating one the first time.
    ///
    /// The key lives in `_settings` so every node signs with the same key; a
    /// node that started with its own would reject the others' tokens.
    pub async fn load(&self) -> Result<()> {
        let client = self.db.client().await?;
        let keys = match read_key(&client).await? {
            Some(der) => SigningKeys::from_der(&der)?,
            None => {
                let keys = SigningKeys::generate()?;
                write_key(&client, &keys).await?;
                tracing::info!(kid = keys.kid(), "generated an access-token signing key");
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

        self.issue(Subject::Superuser, &account.id, "", "", &account.token_key)
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
                    "select \"id\", \"password_hash\", \"token_key\" from {table} \
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
        let token_key: String = row
            .get::<_, Option<String>>("token_key")
            .unwrap_or_default();
        self.issue(
            Subject::Auth,
            &id,
            &collection.id,
            &collection.name,
            &token_key,
        )
        .await
    }

    /// Exchange a refresh token for a new token pair.
    ///
    /// The account is re-read, so a deleted record, a removed collection or a
    /// changed token key ends the session here rather than at access-token expiry.
    pub async fn refresh(
        &self,
        refresh_token: &str,
        collections: &[Collection],
    ) -> Result<AuthOutcome> {
        let client = self.db.client().await?;

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
    /// Done on every refresh, so a deleted record, a removed collection or a
    /// changed token key ends the session here rather than at access-token expiry.
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
                &format!("select \"token_key\" from {table} where \"id\" = $1::text"),
                &[&session.user_id],
            )
            .await
            .map_err(db_error)?
            .ok_or_else(|| Error::unauthorized("the account no longer exists"))?;

        Ok(Account {
            subject: Subject::Auth,
            id: session.user_id.clone(),
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
        collection_id: &str,
        collection_name: &str,
        token_key: &str,
    ) -> Result<String> {
        let keys = self.keys()?;
        let claims = keys.claims_for(subject, id, collection_id, collection_name, token_key)?;
        keys.sign(&claims)
    }

    async fn issue(
        &self,
        subject: Subject,
        id: &str,
        collection_id: &str,
        collection_name: &str,
        token_key: &str,
    ) -> Result<AuthOutcome> {
        let client = self.db.client().await?;
        let (refresh_token, _session) =
            sessions::issue(&client, collection_id, id, None, None).await?;

        Ok(AuthOutcome {
            access_token: self.sign(subject, id, collection_id, collection_name, token_key)?,
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

async fn read_key(runner: &dyn QueryRunner) -> Result<Option<Vec<u8>>> {
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
    let der = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| Error::config("the stored signing key is not valid base64"))?;
    Ok(Some(der))
}

async fn write_key(runner: &dyn QueryRunner, keys: &SigningKeys) -> Result<()> {
    let value = json!({
        "kid": keys.kid(),
        "der": base64::engine::general_purpose::STANDARD.encode(keys.to_der()),
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
