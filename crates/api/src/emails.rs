//! The email flows: verification, password reset and email change (PLAN.md 7.1).
//!
//! Each flow has a *request* half that mints a one-time token into `_otps` and
//! emails it, and a *confirm* half that looks the token up and applies the change.
//! The reference is `main`'s `Lazypock.Emails` + `EmailController`.
//!
//! Two decisions worth stating:
//!
//! * **The token is 32 random bytes, stored as its SHA-256.** It is not a
//!   user-chosen secret, so there is nothing to brute-force and no KDF is needed;
//!   hashing it directly is what lets the confirm path be one indexed read rather
//!   than a slow verification against every outstanding row.
//! * **The request half always answers `204`.** Whether or not the address has an
//!   account must not be observable, or these endpoints become a way to enumerate
//!   users.
//!
//! The change itself is applied through the owner connection, not under a request
//! role: this is a server action, and the account it acts on is the one the token
//! names.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use lazypock_auth::password;
use lazypock_core::error::Error;
use lazypock_core::id::new_id;
use lazypock_db::collections::Collection;
use lazypock_db::ddl::quote_ident;
use lazypock_db::deadpool_postgres::Client;
use lazypock_db::pool::db_error;
use lazypock_db::query::compile::FilterContext;
use lazypock_db::query::projection::{ExpandSpec, FieldSelection};
use lazypock_db::records;
use lazypock_db::types::FieldType;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::auth::Principal;
use crate::error::ApiError;
use crate::jobs;
use crate::records::resolve;
use crate::state::AppState;

/// How long a one-time token stays valid.
pub const TOKEN_TTL_SECONDS: f64 = 24.0 * 60.0 * 60.0;

/// The shortest password `confirm-password-reset` accepts.
const MIN_PASSWORD_LEN: usize = 8;

/// A fresh opaque token: 32 random bytes, url-safe so it survives an email link.
fn new_token() -> Result<String, ApiError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|err| ApiError::new(Error::internal(format!("cannot generate a token: {err}"))))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// The stored form of a token. Hex, so it compares and indexes as text.
fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Mint a token for `record_ref`, mailed to `sent_to`. Returns the raw token.
async fn mint(
    client: &Client,
    collection: &str,
    record_ref: &str,
    sent_to: &str,
) -> Result<String, ApiError> {
    // Sweep expired tokens for this collection first, so an abandoned request cannot
    // leave a row behind forever.
    client
        .execute(
            "delete from _otps \
              where collection_ref = $1 and created_at < now() - make_interval(secs => $2)",
            &[&collection.to_string(), &TOKEN_TTL_SECONDS],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let raw = new_token()?;
    client
        .execute(
            "insert into _otps (id, collection_ref, record_ref, password_hash, sent_to) \
             values ($1, $2, $3, $4, $5)",
            &[
                &new_id(),
                &collection.to_string(),
                &record_ref.to_string(),
                &hash_token(&raw),
                &sent_to.to_string(),
            ],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(raw)
}

/// A token's row.
struct Otp {
    /// The record the token was minted for.
    record_ref: String,
    /// The address it was mailed to; the new address for an email change.
    sent_to: Option<String>,
}

fn otp(row: &lazypock_db::tokio_postgres::Row) -> Otp {
    Otp {
        record_ref: row.get("record_ref"),
        sent_to: row.get("sent_to"),
    }
}

/// Read a token without consuming it.
///
/// Used where the token must survive a failed check — a mistyped password on an
/// email change must not burn the link — before the caller consumes it.
async fn lookup(client: &Client, collection: &str, token: &str) -> Result<Option<Otp>, ApiError> {
    let row = client
        .query_opt(
            "select record_ref, sent_to from _otps \
              where collection_ref = $1 and password_hash = $2 \
                and created_at >= now() - make_interval(secs => $3)",
            &[
                &collection.to_string(),
                &hash_token(token),
                &TOKEN_TTL_SECONDS,
            ],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;
    Ok(row.as_ref().map(otp))
}

/// Consume a token, if it matches and has not expired. Single-use: the row is
/// deleted by the same statement that reads it.
async fn consume(client: &Client, collection: &str, token: &str) -> Result<Option<Otp>, ApiError> {
    let row = client
        .query_opt(
            "delete from _otps \
              where collection_ref = $1 and password_hash = $2 \
                and created_at >= now() - make_interval(secs => $3) \
             returning record_ref, sent_to",
            &[
                &collection.to_string(),
                &hash_token(token),
                &TOKEN_TTL_SECONDS,
            ],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;
    Ok(row.as_ref().map(otp))
}

fn require_auth_collection(collection: &Collection) -> Result<(), ApiError> {
    if collection.collection_type.is_auth() {
        Ok(())
    } else {
        Err(ApiError::new(Error::not_found(format!(
            "no auth collection named {:?}",
            collection.name
        ))))
    }
}

fn email_field(collection: &Collection) -> String {
    collection
        .fields
        .iter()
        .find(|field| field.field_type == FieldType::Email)
        .map(|field| field.name.clone())
        .unwrap_or_else(|| "email".to_string())
}

fn password_field(collection: &Collection) -> String {
    collection
        .fields
        .iter()
        .find(|field| field.field_type == FieldType::Password)
        .map(|field| field.name.clone())
        .unwrap_or_else(|| "password_hash".to_string())
}

/// The id and display name of the account with this email, if there is one.
async fn find_account(
    client: &Client,
    collection: &Collection,
    email: &str,
) -> Result<Option<(String, String)>, ApiError> {
    let table = quote_ident(&collection.name);
    let field = quote_ident(&email_field(collection));
    let has_name = collection.fields.iter().any(|field| field.name == "name");
    let sql = if has_name {
        format!("select \"id\", \"name\" from {table} where lower({field}) = lower($1::text)")
    } else {
        format!("select \"id\" from {table} where lower({field}) = lower($1::text)")
    };

    let row = client
        .query_opt(&sql, &[&email.to_string()])
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(row.map(|row| {
        let id: String = row.get("id");
        let name: Option<String> = if has_name { row.get("name") } else { None };
        let name = name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| email.to_string());
        (id, name)
    }))
}

async fn stored_password(
    client: &Client,
    collection: &Collection,
    id: &str,
) -> Result<Option<String>, ApiError> {
    let table = quote_ident(&collection.name);
    let field = password_field(collection);
    let row = client
        .query_opt(
            &format!(
                "select {} from {table} where \"id\" = $1::text",
                quote_ident(&field)
            ),
            &[&id.to_string()],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;
    Ok(row.and_then(|row| row.get::<_, Option<String>>(field.as_str())))
}

/// The record the token names, as a client reads it — credentials excluded by the
/// projection, as everywhere else.
async fn load_account(
    client: &Client,
    collection: &Collection,
    context: &FilterContext,
    id: &str,
) -> Result<Value, ApiError> {
    records::get_one(
        client,
        collection,
        context,
        id,
        &FieldSelection::all(),
        &ExpandSpec::none(),
    )
    .await
    .map_err(ApiError::new)?
    .ok_or_else(|| ApiError::new(Error::not_found("the account no longer exists")))
}

/// The signed-in account, checked to belong to this collection.
fn require_member(collection: &str, principal: &Principal) -> Result<(String, String), ApiError> {
    let claims = principal
        .claims()
        .filter(|claims| !claims.typ.is_superuser())
        .ok_or_else(|| {
            ApiError::new(Error::unauthorized("this action needs a signed-in account"))
        })?;
    if claims.name != collection {
        return Err(ApiError::new(Error::forbidden(
            "the token belongs to a different collection",
        )));
    }
    Ok((claims.sub.clone(), claims.email.clone()))
}

/// The shared half of `request-verification` and `request-password-reset`.
///
/// Always answers `204`, whether or not the address has an account.
async fn request_email(
    state: &AppState,
    collection: &str,
    email: &str,
    template: &str,
) -> Result<Response, ApiError> {
    let (_schema, resolved) = resolve(state, collection)?;
    require_auth_collection(&resolved.collection)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    if let Some((id, name)) = find_account(&client, &resolved.collection, email).await? {
        let token = mint(&client, collection, &id, email).await?;
        jobs::enqueue(
            &client,
            jobs::mail_job(template, &name, email, json!({ "token": token })),
        )
        .await?;
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// What `request-verification` and `request-password-reset` accept.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailRequest {
    /// The address to send to.
    pub email: String,
}

/// What `confirm-verification` accepts.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenRequest {
    /// The token from the email.
    pub token: String,
}

/// `POST /api/collections/{collection}/request-verification`
pub async fn request_verification(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    body: Result<Json<EmailRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = crate::auth::json_body(body)?;
    request_email(&state, &collection, &request.email, "verification").await
}

/// `POST /api/collections/{collection}/confirm-verification`
pub async fn confirm_verification(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    body: Result<Json<TokenRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = crate::auth::json_body(body)?;
    let (_schema, resolved) = resolve(&state, &collection)?;
    require_auth_collection(&resolved.collection)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    let otp = consume(&client, &collection, &request.token)
        .await?
        .ok_or_else(|| ApiError::new(Error::invalid("invalid or expired token")))?;

    let table = quote_ident(&collection);
    client
        .execute(
            &format!(
                "update {table} set \"verified\" = true, \"updated\" = now() \
                  where \"id\" = $1::text"
            ),
            &[&otp.record_ref],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let record = load_account(
        &client,
        &resolved.collection,
        &resolved.context,
        &otp.record_ref,
    )
    .await?;
    Ok((StatusCode::OK, Json(json!({ "record": record }))).into_response())
}

/// `POST /api/collections/{collection}/request-password-reset`
pub async fn request_password_reset(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    body: Result<Json<EmailRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = crate::auth::json_body(body)?;
    request_email(&state, &collection, &request.email, "password_reset").await
}

/// What `confirm-password-reset` accepts.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewPassword {
    /// The token from the email.
    pub token: String,
    /// The new password.
    pub password: String,
    /// The new password again.
    pub password_confirm: Option<String>,
}

/// `POST /api/collections/{collection}/confirm-password-reset`
pub async fn confirm_password_reset(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    body: Result<Json<NewPassword>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = crate::auth::json_body(body)?;
    validate_password(&request.password, request.password_confirm.as_deref())?;

    let (_schema, resolved) = resolve(&state, &collection)?;
    require_auth_collection(&resolved.collection)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    let otp = consume(&client, &collection, &request.token)
        .await?
        .ok_or_else(|| ApiError::new(Error::invalid("invalid or expired token")))?;

    let hashed = password::hash(&request.password).map_err(ApiError::new)?;
    let table = quote_ident(&collection);
    let field = quote_ident(&password_field(&resolved.collection));
    client
        .execute(
            &format!(
                "update {table} set {field} = $1, \"updated\" = now() where \"id\" = $2::text"
            ),
            &[&hashed, &otp.record_ref],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    // A reset ends the sessions the old password created, or a stolen token would
    // keep working after the owner changed it.
    lazypock_auth::sessions::revoke_for_user(&client, &resolved.collection.id, &otp.record_ref)
        .await
        .map_err(ApiError::new)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

fn validate_password(password: &str, confirm: Option<&str>) -> Result<(), ApiError> {
    if password.is_empty() {
        return Err(ApiError::new(Error::invalid("A password is required")));
    }
    if confirm != Some(password) {
        return Err(ApiError::new(Error::invalid("Passwords do not match")));
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ApiError::new(Error::invalid(format!(
            "A password must be at least {MIN_PASSWORD_LEN} characters"
        ))));
    }
    Ok(())
}

/// What `request-email-change` accepts.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmailChange {
    /// The address to move to.
    pub new_email: String,
}

/// `POST /api/collections/{collection}/request-email-change`
pub async fn request_email_change(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<EmailChange>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = crate::auth::json_body(body)?;
    let (_schema, resolved) = resolve(&state, &collection)?;
    require_auth_collection(&resolved.collection)?;
    let (user_id, _email) = require_member(&collection, &principal)?;

    let new_email = request.new_email.trim().to_string();
    if new_email.is_empty() {
        return Err(ApiError::new(Error::invalid("a `newEmail` is required")));
    }

    let client = state.db.client().await.map_err(ApiError::new)?;
    // The token is stored against the *new* address, so the confirm half reads the
    // destination from the row rather than trusting the caller.
    let token = mint(&client, &collection, &user_id, &new_email).await?;
    jobs::enqueue(
        &client,
        jobs::mail_job(
            "email_change",
            &new_email,
            &new_email,
            json!({ "token": token, "email": new_email }),
        ),
    )
    .await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// What `confirm-email-change` accepts.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmEmailChange {
    /// The token from the email.
    pub token: String,
    /// The account's current password.
    pub password: String,
}

/// `POST /api/collections/{collection}/confirm-email-change`
pub async fn confirm_email_change(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<ConfirmEmailChange>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = crate::auth::json_body(body)?;
    let (_schema, resolved) = resolve(&state, &collection)?;
    require_auth_collection(&resolved.collection)?;
    let (user_id, _email) = require_member(&collection, &principal)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    // Read the token without consuming it first: the password check below can fail,
    // and a mistyped password must not burn the link.
    let found = lookup(&client, &collection, &request.token)
        .await?
        .ok_or_else(|| ApiError::new(Error::invalid("invalid or expired token")))?;

    // The token must be this account's: one minted for another record must not let
    // an account change somebody else's address.
    if found.record_ref != user_id {
        return Err(ApiError::new(Error::invalid("invalid token")));
    }

    // Moving the address is credential-adjacent, so it also needs the password.
    let stored = stored_password(&client, &resolved.collection, &user_id).await?;
    let ok = stored
        .as_deref()
        .map(|stored| password::verify(&request.password, stored))
        .unwrap_or(false);
    if !ok {
        return Err(ApiError::new(Error::unauthorized("invalid password")));
    }

    // Checks passed: claim it atomically now.
    let otp = consume(&client, &collection, &request.token)
        .await?
        .ok_or_else(|| ApiError::new(Error::invalid("invalid or expired token")))?;

    let new_email = otp
        .sent_to
        .ok_or_else(|| ApiError::new(Error::internal("the token has no destination")))?;
    let table = quote_ident(&collection);
    let field = quote_ident(&email_field(&resolved.collection));
    client
        .execute(
            &format!(
                "update {table} set {field} = $1, \"updated\" = now() where \"id\" = $2::text"
            ),
            &[&new_email, &user_id],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let record = load_account(&client, &resolved.collection, &resolved.context, &user_id).await?;
    Ok((StatusCode::OK, Json(json!({ "record": record }))).into_response())
}
