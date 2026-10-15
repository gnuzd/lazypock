//! Authentication endpoints and the request middleware.
//!
//! A missing bearer token means an anonymous request. A token that is *present*
//! but invalid — wrong signature, expired, malformed — is a `401` rather than a
//! silent downgrade to anonymous, so a client with a dead token finds out
//! instead of quietly losing access.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lazypock_auth::token::Claims;
use lazypock_auth::AuthOutcome;
use lazypock_core::error::Error;
use lazypock_db::collections::Collection;
use lazypock_db::query::compile::AuthContext;
use lazypock_db::query::projection::{ExpandSpec, FieldSelection};
use lazypock_db::records;
use lazypock_db::rls::context::{Action, RequestContext};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::records::resolve;
use crate::state::AppState;

/// PocketBase's slug for superuser auth.
///
/// It can never collide with a real collection: identifiers may not start with
/// an underscore.
pub const SUPERUSERS: &str = "_superusers";

/// The authenticated caller, attached to every request.
#[derive(Debug, Clone, Default)]
pub struct Principal {
    claims: Option<Claims>,
    /// Whether this caller authenticated with an API key rather than a token.
    api_key: bool,
    /// The client's device/tab id, used by realtime to suppress echoes.
    connection_id: String,
}

impl Principal {
    /// Nobody is authenticated.
    pub fn anonymous() -> Self {
        Self {
            claims: None,
            api_key: false,
            connection_id: String::new(),
        }
    }

    /// A caller with verified claims.
    pub fn from_claims(claims: Claims) -> Self {
        Self {
            claims: Some(claims),
            api_key: false,
            connection_id: String::new(),
        }
    }

    /// A caller authenticated by an API key.
    ///
    /// Deliberately not a superuser and with no record identity: an API key is a
    /// scoped credential, so it carries no superuser bypass and no
    /// `@request.auth.*`. Endpoints that accept it check [`Principal::is_api_key`]
    /// explicitly.
    pub fn from_api_key() -> Self {
        Self {
            claims: None,
            api_key: true,
            connection_id: String::new(),
        }
    }

    /// Attach the client's connection id.
    pub fn with_connection_id(mut self, connection_id: impl Into<String>) -> Self {
        self.connection_id = connection_id.into();
        self
    }

    /// The row-level-security context this request must run under.
    ///
    /// A superuser runs as the owner, which is exactly what makes a superuser
    /// bypass the collection rules; an authenticated record runs as the
    /// restricted role with its identity attached; anyone else is anonymous, so
    /// `@request.auth.*` resolves to nothing and a rule depending on it matches
    /// nothing.
    ///
    /// `action` is the record operation the request performs, so a `SELECT` is
    /// matched against that operation's rule rather than the OR of `listRule`
    /// and `viewRule` (see [`Action`]).
    pub fn request_context(&self, action: Action) -> RequestContext {
        let context = match &self.claims {
            Some(claims) if claims.typ.is_superuser() => RequestContext::owner(),
            Some(claims) => {
                RequestContext::authenticated(&claims.sub, &claims.email, &claims.col, &claims.name)
            }
            None => RequestContext::anonymous(),
        };
        context
            .with_connection_id(self.connection_id.clone())
            .with_action(action)
    }

    /// The verified claims, when there are any.
    pub fn claims(&self) -> Option<&Claims> {
        self.claims.as_ref()
    }

    /// Whether this caller bypasses access rules.
    pub fn is_superuser(&self) -> bool {
        self.claims
            .as_ref()
            .is_some_and(|claims| claims.typ.is_superuser())
    }

    /// Whether this caller presented a valid API key.
    pub fn is_api_key(&self) -> bool {
        self.api_key
    }

    /// The authenticated account id.
    pub fn user_id(&self) -> Option<&str> {
        self.claims.as_ref().map(|claims| claims.sub.as_str())
    }

    /// The claims the query layer understands.
    ///
    /// An anonymous caller becomes an empty context, so `@request.auth.*`
    /// resolves to nothing and filters match nothing rather than everything.
    pub fn auth_context(&self) -> AuthContext {
        match &self.claims {
            Some(claims) => AuthContext {
                id: Some(claims.sub.clone()),
                email: Some(claims.email.clone()),
                collection_id: non_empty(&claims.col),
                collection_name: non_empty(&claims.name),
                claims: Default::default(),
            },
            None => AuthContext::default(),
        }
    }
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Attach the caller to the request.
pub async fn authenticate(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // `auth-refresh` carries the *refresh* token as a bearer — that is how
    // PocketBase's SDK sends it — and a refresh token is deliberately not a
    // valid access token. Verifying it belongs to that handler, so the
    // middleware only marks the request anonymous here.
    if request.uri().path().ends_with("/auth-refresh") {
        request.extensions_mut().insert(Principal::anonymous());
        return next.run(request).await;
    }

    // A key may travel in the query string as well as the bearer header. Take it
    // out before the handler runs: a strict query parser (`GET /api/collections`
    // denies unknown fields) would otherwise reject `api_key` as a bad parameter.
    let query_key = api_key_from_query(request.uri());
    if let Some(uri) = query_key
        .as_ref()
        .and_then(|_| strip_query_param(request.uri(), "api_key"))
    {
        *request.uri_mut() = uri;
    }

    let principal = match bearer_token(request.headers())
        .map(str::to_string)
        .or_else(|| query_key.clone())
    {
        None => Principal::anonymous(),
        // Only a value carrying the key prefix is looked up, so the common JWT
        // path never pays for a database round trip.
        Some(token) if token.starts_with(crate::api_keys::PREFIX) => {
            let client = match state.db.client().await {
                Ok(client) => client,
                Err(err) => return ApiError::new(err).into_response(),
            };
            match crate::api_keys::verify(&client, &token).await {
                Ok(true) => Principal::from_api_key(),
                Ok(false) => Principal::anonymous(),
                Err(err) => return err.into_response(),
            }
        }
        Some(token) => match state.auth.verify(&token) {
            Ok(claims) => Principal::from_claims(claims),
            Err(err) => return ApiError::new(err).into_response(),
        },
    }
    .with_connection_id(connection_id(request.headers()));

    request.extensions_mut().insert(principal);
    next.run(request).await
}

/// The client's device/tab id from `X-Connection-Id`.
///
/// Not a secret: it only lets the server avoid echoing a change back to the
/// device that made it. Length-capped and ignored if it is unusable.
pub fn connection_id(headers: &HeaderMap) -> String {
    headers
        .get("x-connection-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .unwrap_or_default()
        .to_string()
}

/// Pull the bearer token out of the `Authorization` header.
pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
        Some(token)
    } else {
        None
    }
}

/// Body for the password login endpoints.
#[derive(Debug, Deserialize)]
pub struct PasswordAuth {
    /// Email address (PocketBase also accepts a username here).
    pub identity: String,
    /// The password.
    pub password: String,
}

/// Body for `auth-refresh`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAuth {
    /// The refresh token, when it is sent in the body rather than as a bearer.
    pub refresh_token: Option<String>,
}

/// `POST /api/collections/{collection}/auth-with-password`
///
/// `GET /api/me`
///
/// The current identity, in the record-like shape a client expects: `id`, `email`
/// and, for a collection user, the collection it belongs to. A superuser has no
/// collection, and that absence is how a client tells the two apart — the same
/// distinction the token itself makes.
///
/// Answered from the token's claims rather than a database read, so it stays cheap
/// enough to call on start-up, and so an identity that has since been deleted
/// still reports itself rather than 404ing on the way to being rejected.
pub async fn me(Extension(principal): Extension<Principal>) -> Result<Json<Value>, ApiError> {
    let Some(claims) = principal.claims() else {
        return Err(ApiError::new(Error::unauthorized(
            "an access token is required",
        )));
    };

    let mut body = json!({
        "id": claims.sub,
        "email": claims.email,
        "superuser": claims.typ.is_superuser(),
    });

    if !claims.typ.is_superuser() {
        if let Some(object) = body.as_object_mut() {
            object.insert("collectionId".to_string(), json!(claims.col));
            object.insert("collectionName".to_string(), json!(claims.name));
        }
    }

    Ok(Json(body))
}

/// Also serves `POST /api/collections/_superusers/auth-with-password`, which is
/// how PocketBase spells superuser login.
pub async fn auth_with_password(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    body: Result<Json<PasswordAuth>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = json_body(body)?;

    if collection == SUPERUSERS {
        let outcome = state
            .auth
            .authenticate_superuser(&request.identity, &request.password)
            .await?;
        let client = state.db.client().await.map_err(ApiError::new)?;
        let account = lazypock_auth::superuser::find_by_id(&client, &outcome.user_id)
            .await
            .map_err(ApiError::new)?
            .ok_or_else(|| {
                ApiError::new(Error::internal("the authenticated superuser vanished"))
            })?;
        // The hash never leaves the server.
        let record = json!({
            "id": account.id,
            "email": account.email,
            "collectionName": SUPERUSERS,
        });
        return Ok((StatusCode::OK, Json(auth_body(&outcome, record))).into_response());
    }

    let (schema, resolved) = resolve(&state, &collection)?;
    if !resolved.collection.collection_type.is_auth() {
        return Err(ApiError::new(Error::not_found(format!(
            "no auth collection named {collection:?}"
        ))));
    }

    let outcome = state
        .auth
        .authenticate_record(&resolved.collection, &request.identity, &request.password)
        .await?;

    // Return the record the client just authenticated as.
    let client = state.db.client().await.map_err(ApiError::new)?;
    let record = records::get_one(
        &client,
        &resolved.collection,
        &resolved.context,
        &outcome.user_id,
        &FieldSelection::all(),
        &ExpandSpec::none(),
    )
    .await
    .map_err(ApiError::new)?
    .ok_or_else(|| ApiError::new(Error::internal("the authenticated record vanished")))?;

    // Touch the schema snapshot so a reload during login is not lost.
    drop(schema);
    Ok((StatusCode::OK, Json(auth_body(&outcome, record))).into_response())
}

/// `POST /api/collections/{collection}/auth-refresh`
///
/// The refresh token may be sent in the body or as a bearer token; PocketBase's
/// SDK sends it as a bearer.
pub async fn auth_refresh(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    headers: HeaderMap,
    body: Result<Json<RefreshAuth>, JsonRejection>,
) -> Result<Response, ApiError> {
    let request = body.map(|Json(request)| request).unwrap_or_default();
    let token = request
        .refresh_token
        .or_else(|| bearer_token(&headers).map(str::to_string))
        .ok_or_else(|| {
            ApiError::new(Error::invalid(
                "a refresh token is required, in the body or as a bearer token",
            ))
        })?;

    let schema = state.schema.snapshot();
    let outcome = state.auth.refresh(&token, schema.collections()).await?;

    // A token issued for one collection must not be usable against another.
    if !outcome.collection_id.is_empty() && outcome.collection_name != collection {
        return Err(ApiError::new(Error::unauthorized(
            "the refresh token belongs to a different collection",
        )));
    }

    let client = state.db.client().await.map_err(ApiError::new)?;
    let record = if outcome.collection_id.is_empty() {
        let account = lazypock_auth::superuser::find_by_id(&client, &outcome.user_id)
            .await
            .map_err(ApiError::new)?
            .ok_or_else(|| ApiError::new(Error::internal("the account vanished")))?;
        json!({ "id": account.id, "email": account.email, "collectionName": SUPERUSERS })
    } else {
        let resolved_collection = schema
            .get(&outcome.collection_id)
            .cloned()
            .ok_or_else(|| ApiError::new(Error::unauthorized("the collection no longer exists")))?;
        let context = lazypock_db::query::compile::FilterContext::for_collection(
            &resolved_collection,
            schema.collections(),
        )
        .map_err(ApiError::new)?;
        records::get_one(
            &client,
            &resolved_collection,
            &context,
            &outcome.user_id,
            &FieldSelection::all(),
            &ExpandSpec::none(),
        )
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| ApiError::new(Error::internal("the account vanished")))?
    };

    Ok((StatusCode::OK, Json(auth_body(&outcome, record))).into_response())
}

/// `GET /api/collections/{collection}/auth-methods`
pub async fn auth_methods(
    State(state): State<AppState>,
    Path(collection): Path<String>,
) -> Result<Response, ApiError> {
    if collection != SUPERUSERS {
        let (_schema, resolved) = resolve(&state, &collection)?;
        if !resolved.collection.collection_type.is_auth() {
            return Err(ApiError::new(Error::not_found(format!(
                "no auth collection named {collection:?}"
            ))));
        }
    }

    // Only password auth exists so far; the others are reported as disabled
    // rather than omitted, so a client can tell "off" from "unknown".
    Ok((
        StatusCode::OK,
        Json(json!({
            "password": { "enabled": true },
            "oauth2": { "enabled": false, "providers": [] },
            "otp": { "enabled": false },
            "mfa": { "enabled": false },
        })),
    )
        .into_response())
}

/// `GET /api/jwks` and `GET /.well-known/jwks.json`
pub async fn jwks(State(state): State<AppState>) -> Result<Response, ApiError> {
    let jwks = state.auth.jwks().map_err(ApiError::new)?;
    Ok((StatusCode::OK, Json(jwks)).into_response())
}

pub(crate) fn auth_body(outcome: &AuthOutcome, record: Value) -> Value {
    json!({
        "token": outcome.access_token,
        "refreshToken": outcome.refresh_token,
        "expiresIn": outcome.expires_in,
        "record": record,
    })
}

/// Validate before hashing.
///
/// The hasher rejects a short password with a generic failure; running the field
/// validation first means the client still gets the usual per-field detail. The
/// record layer validates again, which is harmless.
/// Validate a request body before a transaction is opened.
///
/// `partial` skips the required-field check, and a caller passing `true` is saying
/// "something may fill an absent field in later" — true for an update, and true
/// for a create with hooks. The authoritative check always runs in the record
/// layer, so this is an early, cheaper report rather than the only guard.
/// An extractor that admits only a superuser.
///
/// It runs among the **parts** extractors, so it rejects before a body extractor has
/// read anything. Checking inside a handler body instead means the body extractor
/// runs first, and an anonymous caller with no multipart body gets the body error
/// rather than the authorization error — which is exactly how `/api/import` answered
/// 415 where it should have answered 403.
#[derive(Debug, Clone, Copy)]
pub struct Superuser;

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Superuser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let principal = parts
            .extensions
            .get::<Principal>()
            .cloned()
            .unwrap_or_else(Principal::anonymous);
        require_superuser(&principal)?;
        Ok(Superuser)
    }
}

/// The `api_key` query parameter, if present.
///
/// PocketBase's SDK can carry a key in either place; the value is the raw key, so
/// no percent-decoding is needed for the characters it can contain.
fn api_key_from_query(uri: &axum::http::Uri) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == "api_key").then(|| value.to_string())
    })
}

/// The same URI with one query parameter removed.
fn strip_query_param(uri: &axum::http::Uri, name: &str) -> Option<axum::http::Uri> {
    let kept: Vec<&str> = uri
        .query()?
        .split('&')
        .filter(|pair| {
            pair.split_once('=')
                .map_or(*pair != name, |(key, _)| key != name)
        })
        .collect();

    let target = if kept.is_empty() {
        uri.path().to_string()
    } else {
        format!("{}?{}", uri.path(), kept.join("&"))
    };

    let mut parts = uri.clone().into_parts();
    parts.path_and_query = Some(axum::http::uri::PathAndQuery::from_maybe_shared(target).ok()?);
    axum::http::Uri::from_parts(parts).ok()
}

/// Fail unless the caller is a superuser.
///
/// One implementation for every administrative surface: configuration is
/// superuser-only, and places that enforce it separately are places that can
/// disagree.
pub fn require_superuser(principal: &Principal) -> Result<(), ApiError> {
    if principal.is_superuser() {
        Ok(())
    } else {
        Err(ApiError::new(Error::forbidden(
            "this action requires a superuser token",
        )))
    }
}

/// Fail unless the caller is a superuser or a valid API key.
///
/// Used only by the collection reads a codegen client needs. An API key stays
/// refused everywhere [`require_superuser`] is used, so granting it here cannot
/// widen it by accident.
pub fn require_superuser_or_api_key(principal: &Principal) -> Result<(), ApiError> {
    if principal.is_superuser() || principal.is_api_key() {
        Ok(())
    } else {
        Err(ApiError::new(Error::forbidden(
            "this action requires a superuser token or an API key",
        )))
    }
}

pub fn validate_first(
    collection: &Collection,
    input: &Value,
    partial: bool,
) -> Result<(), ApiError> {
    let Some(object) = input.as_object() else {
        // A non-object body is reported by the record layer.
        return Ok(());
    };
    lazypock_db::validation::validate(collection, object, partial).map_err(|errors| {
        ApiError::from(lazypock_db::records::RecordWriteError::Validation(errors))
    })
}

/// The server-managed columns an auth-collection write needs.
///
/// The password is not a column: it is hashed *here*, which is why the record layer
/// is handed the result rather than doing the hashing itself — hashing lives in
/// the auth crate, and that crate depends on the database layer, so the inverse
/// would be a cycle. It is read under either spelling the client may use —
/// PocketBase's `password` or the collection's own password field name — by
/// [`Collection::password_input`].
///
/// A new password also rotates the account's `token_key`. What ends the tokens
/// issued before it is the caller revoking the account's sessions
/// (`sessions::revoke_for_user`), which is what makes the old refresh token fail; the
/// rotation itself is carried into the next access token but never verified.
pub fn auth_columns(
    collection: &Collection,
    input: &Value,
) -> Result<Vec<(String, Option<String>)>, ApiError> {
    if !collection.collection_type.is_auth() {
        return Ok(Vec::new());
    }
    let Some(password) = input
        .as_object()
        .and_then(|body| collection.password_input(body))
        .and_then(Value::as_str)
    else {
        return Ok(Vec::new());
    };
    let Some(field) = collection.password_field() else {
        return Ok(Vec::new());
    };

    Ok(vec![
        (
            field.name.clone(),
            Some(lazypock_auth::password::hash(password).map_err(ApiError::new)?),
        ),
        (
            "token_key".to_string(),
            Some(lazypock_auth::sessions::generate_token().map_err(ApiError::new)?),
        ),
    ])
}

pub(crate) fn json_body<T: serde::de::DeserializeOwned>(
    body: Result<Json<T>, JsonRejection>,
) -> Result<Json<T>, ApiError> {
    body.map_err(|rejection| {
        ApiError::new(Error::invalid(format!(
            "invalid JSON body: {}",
            rejection.body_text()
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn bearer_tokens_are_extracted_case_insensitively() {
        let mut headers = HeaderMap::new();
        assert!(bearer_token(&headers).is_none());

        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc123"),
        );
        assert_eq!(bearer_token(&headers), Some("abc123"));

        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer   spaced  "),
        );
        assert_eq!(bearer_token(&headers), Some("spaced"));

        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic abc123"),
        );
        assert!(bearer_token(&headers).is_none());

        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer   "));
        assert!(bearer_token(&headers).is_none());
    }

    #[test]
    fn an_anonymous_principal_has_no_claims() {
        let principal = Principal::anonymous();
        assert!(!principal.is_superuser());
        assert!(principal.user_id().is_none());
        let context = principal.auth_context();
        assert_eq!(context.id, None);
        assert_eq!(context.email, None);
    }
}
