//! Authentication endpoints and the request middleware.
//!
//! A missing bearer token means an anonymous request. A token that is *present*
//! but invalid — wrong signature, expired, malformed — is a `401` rather than a
//! silent downgrade to anonymous, so a client with a dead token finds out
//! instead of quietly losing access.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use lazypock_auth::token::Claims;
use lazypock_auth::AuthOutcome;
use lazypock_core::error::Error;
use lazypock_db::collections::Collection;
use lazypock_db::hooks::{HookContext, ScopedHooks};
use lazypock_db::query::compile::AuthContext;
use lazypock_db::query::projection::{ExpandSpec, FieldSelection};
use lazypock_db::records;
use lazypock_db::rls::context::{Action, RequestContext};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

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
    /// The authenticated record's own columns, for `@request.auth.<field>`.
    ///
    /// Loaded once per authenticated request (see [`record_claims`]) because a rule
    /// is evaluated in SQL, where the record is not otherwise reachable: publishing
    /// the columns on the session is what lets `@request.auth.role = 'admin'` be
    /// true for an account whose `role` is `admin`.
    record_claims: std::collections::BTreeMap<String, String>,
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
            record_claims: std::collections::BTreeMap::new(),
            api_key: false,
            connection_id: String::new(),
        }
    }

    /// A caller with verified claims.
    pub fn from_claims(claims: Claims) -> Self {
        Self {
            claims: Some(claims),
            record_claims: std::collections::BTreeMap::new(),
            api_key: false,
            connection_id: String::new(),
        }
    }

    /// Attach the authenticated record's own columns.
    pub fn with_record_claims(
        mut self,
        claims: std::collections::BTreeMap<String, String>,
    ) -> Self {
        self.record_claims = claims;
        self
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
            record_claims: std::collections::BTreeMap::new(),
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
            .with_auth_claims(
                serde_json::to_string(&self.record_claims).unwrap_or_else(|_| "{}".to_string()),
            )
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
                claims: self
                    .record_claims
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect(),
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
                // A value carrying the key prefix *is* a key attempt, so "not found" means
                // wrong, revoked or expired — and that is a 401, not a quiet demotion to
                // anonymous. Demoting kept a revoked key working against everything public
                // and made a typo'd key look like a permissions problem.
                Ok(false) => {
                    return ApiError::new(Error::unauthorized(
                        "the API key is not valid, or it has been revoked or has expired",
                    ))
                    .into_response()
                }
                Err(err) => return err.into_response(),
            }
        }
        Some(token) => match state.auth.verify(&token) {
            Ok(claims) => {
                let record_claims = record_claims(&state, &claims).await;
                Principal::from_claims(claims).with_record_claims(record_claims)
            }
            Err(err) => return ApiError::new(err).into_response(),
        },
    }
    .with_connection_id(connection_id(request.headers()));

    request.extensions_mut().insert(principal);
    next.run(request).await
}

/// The authenticated record's own columns, for `@request.auth.<field>`.
///
/// One query per authenticated request, and only when a record is behind the token:
/// a superuser and an API key have no record and skip it. The record is read on the
/// connecting (owner) role — this is the server establishing who the caller is, not
/// a caller reading a row — and credential columns are dropped before anything is
/// published to the database session.
///
/// A failure returns no claims rather than failing the request: a rule that depends
/// on them then resolves to nothing, which is the fail-closed reading. The reason is
/// logged, because "the token verifies but the record is gone" is worth seeing.
async fn record_claims(
    state: &AppState,
    claims: &Claims,
) -> std::collections::BTreeMap<String, String> {
    let empty = std::collections::BTreeMap::new();
    if claims.typ.is_superuser() || claims.name.is_empty() || claims.sub.is_empty() {
        return empty;
    }
    // The collection name is signed, but it becomes an identifier in a statement —
    // validated anyway, so nothing about the token is trusted more than it has to be.
    if lazypock_db::types::validate_collection_name(&claims.name).is_err() {
        return empty;
    }
    let Ok(client) = state.db.client().await else {
        tracing::warn!(
            collection = %claims.name,
            "could not load the authenticated record's claims: no database connection"
        );
        return empty;
    };
    let statement = format!(
        "select to_jsonb(t) from {} t where t.id = $1",
        lazypock_db::ddl::quote_ident(&claims.name)
    );
    let row = match client.query_opt(statement.as_str(), &[&claims.sub]).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            tracing::debug!(
                collection = %claims.name,
                "the authenticated record no longer exists; its rules see no claims"
            );
            return empty;
        }
        Err(err) => {
            tracing::warn!(
                collection = %claims.name,
                error = %err,
                "could not load the authenticated record's claims"
            );
            return empty;
        }
    };

    let value: serde_json::Value = row.get(0);
    let mut out = std::collections::BTreeMap::new();
    let Some(object) = value.as_object() else {
        return empty;
    };
    for (name, value) in object {
        // Credentials never become session state, whatever a rule asks for.
        if lazypock_db::types::CREDENTIAL_COLUMNS.contains(&name.as_str()) || value.is_null() {
            continue;
        }
        let text = match value {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        out.insert(name.clone(), text);
    }
    out
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

    // The per-account lockout, checked before the password is looked at. It counts
    // *failures*, so a legitimate sign-in never spends the budget and a success
    // clears it — which is what keeps an attacker who knows an address from
    // locking its owner out. See `rate_limit::Lockout`.
    let account = account_key(&collection, &request.identity);
    if let Err(refused) = state.rate_limits.account.is_locked(&account) {
        let seconds = refused.retry_after.as_secs().max(1);
        tracing::warn!(account = %account, "account locked after failed sign-ins");
        return Ok(crate::rate_limit::too_many(
            seconds,
            format!("Too many failed sign-ins. Try again in {seconds} seconds."),
        ));
    }

    if collection == SUPERUSERS {
        let outcome = record_credential_outcome(
            &state.rate_limits.account,
            &account,
            state
                .auth
                .authenticate_superuser(&request.identity, &request.password)
                .await,
        )?;
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

    let outcome = record_credential_outcome(
        &state.rate_limits.account,
        &account,
        state
            .auth
            .authenticate_record(&resolved.collection, &request.identity, &request.password)
            .await,
    )?;

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

/// The key a lockout counts an account against: the collection and the submitted
/// identity.
///
/// The identity is normalized, so `Ada@Example.com` and `ada@example.com` share a
/// count. A non-existent account is counted the same as a real one, which is what
/// stops the lockout from becoming a user-enumeration oracle.
fn account_key(collection: &str, identity: &str) -> String {
    format!("{collection}:{}", identity.trim().to_ascii_lowercase())
}

/// Count a failed sign-in, or clear the count on success.
///
/// Only a `401` counts. An internal error is not a wrong password, and counting it
/// would let a database blip lock accounts out.
fn record_credential_outcome<T>(
    lockout: &crate::rate_limit::Lockout,
    key: &str,
    result: Result<T, Error>,
) -> Result<T, ApiError> {
    match result {
        Ok(value) => {
            lockout.clear(key);
            Ok(value)
        }
        Err(err) => {
            if err.status() == StatusCode::UNAUTHORIZED.as_u16() {
                lockout.record_failure(key);
            }
            Err(ApiError::new(err))
        }
    }
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
    // A token issued for one collection must not be usable against another, and that check
    // belongs *before* the rotation rather than after it: rotation is destructive, so
    // checking afterwards consumed the token and discarded its replacement, logging the
    // user out because of a client bug. `AuthService::refresh` peeks, compares, then rotates.
    let outcome = state
        .auth
        .refresh(&token, schema.collections(), Some(collection.as_str()))
        .await?;

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
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let is_superusers = collection == SUPERUSERS;
    if !is_superusers {
        let (_schema, resolved) = resolve(&state, &collection)?;
        if !resolved.collection.collection_type.is_auth() {
            return Err(ApiError::new(Error::not_found(format!(
                "no auth collection named {collection:?}"
            ))));
        }
    }

    // OAuth2 is for auth records. The superuser login stays password-only, as the
    // reference's routes are, so `_superusers` lists no providers.
    let providers = if is_superusers {
        Vec::new()
    } else {
        let client = state.db.client().await.map_err(ApiError::new)?;
        let configured = crate::oauth2::configured(&client)
            .await
            .map_err(ApiError::new)?;
        let app_origin = app_origin(&state, &headers);
        let mut listed = Vec::with_capacity(configured.len());
        for provider in &configured {
            let pkce = crate::oauth2::Pkce::new().map_err(ApiError::new)?;
            let auth_url = crate::oauth2::authorize_url(provider, &pkce);
            // Remember where this flow started, so the provider's callback knows
            // which origin may receive the result. Keyed by the state, single-use.
            crate::oauth2::remember_flow(
                &client,
                &collection,
                &pkce.state,
                json!({ "provider": provider.name, "app_origin": app_origin }),
            )
            .await
            .map_err(ApiError::new)?;
            listed.push(json!({
                "name": provider.name,
                "authURL": auth_url,
                "state": pkce.state,
                "codeVerifier": pkce.verifier,
            }));
        }
        listed
    };

    // The other methods are reported as disabled rather than omitted, so a client
    // can tell "off" from "unknown".
    let oauth2_enabled = !providers.is_empty();
    Ok((
        StatusCode::OK,
        Json(json!({
            "password": { "enabled": true },
            "oauth2": { "enabled": oauth2_enabled, "providers": providers },
            "otp": { "enabled": false },
            "mfa": { "enabled": false },
        })),
    )
        .into_response())
}

/// The app origin a popup result may be posted to.
///
/// The browser sends `Origin` on the cross-origin request that starts the flow.
/// It is believed only when the CORS allowlist accepts it; otherwise the
/// deployment's own origin is used, so a same-origin Studio keeps working. A
/// caller cannot redirect the result somewhere else by naming it in the request.
fn app_origin(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        let origin = origin.trim();
        if state.cors.allows(origin) {
            return origin.to_string();
        }
    }

    if let Some(public) = &state.config.public_url {
        return public.trim_end_matches('/').to_string();
    }

    headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(|host| format!("http://{host}"))
        .unwrap_or_default()
}

/// What `POST /api/collections/{collection}/auth-with-oauth2` accepts.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OAuth2Auth {
    /// The provider name, as `auth-methods` listed it.
    pub provider: String,
    /// The authorization code the provider returned to the popup.
    pub code: String,
    /// The PKCE verifier the client kept from `auth-methods`.
    pub code_verifier: String,
    /// The redirect URI used at authorize time; the provider needs the same one.
    pub redirect_url: String,
    /// Extra record fields to set on a first sign-in.
    #[serde(default)]
    pub create_data: Option<Value>,
}

/// `POST /api/collections/{collection}/auth-with-oauth2`
///
pub async fn auth_with_oauth2(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    body: Result<Json<OAuth2Auth>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = json_body(body)?;

    if collection == SUPERUSERS {
        return Err(ApiError::new(Error::not_found(
            "OAuth2 sign-in is for auth collections; the superuser login is password-only",
        )));
    }
    let (_schema, resolved) = resolve(&state, &collection)?;
    if !resolved.collection.collection_type.is_auth() {
        return Err(ApiError::new(Error::not_found(format!(
            "no auth collection named {collection:?}"
        ))));
    }

    let client = state.db.client().await.map_err(ApiError::new)?;
    let providers = crate::oauth2::configured(&client)
        .await
        .map_err(ApiError::new)?;
    let provider = providers
        .iter()
        .find(|provider| provider.name == request.provider)
        .ok_or_else(|| {
            ApiError::new(Error::not_found(format!(
                "no oauth2 provider named {:?}",
                request.provider
            )))
        })?;

    let token = crate::oauth2::exchange(
        provider,
        &request.code,
        &request.code_verifier,
        &request.redirect_url,
    )
    .await
    .map_err(ApiError::new)?;
    let user = crate::oauth2::user(provider, &token)
        .await
        .map_err(ApiError::new)?;

    let record_id =
        match crate::oauth2::find_link(&client, &collection, &provider.name, &user.provider_id)
            .await
            .map_err(ApiError::new)?
        {
            Some(linked) => linked,
            None => {
                let input =
                    oauth2_create_input(&resolved.collection, &user, request.create_data.as_ref());
                validate_first(&resolved.collection, &input, false)?;
                let extra = auth_columns(&resolved.collection, &input)?;
                // A signup is an anonymous request, so that is what a hook sees: this
                // create happens on the owning connection (the account does not exist
                // yet), and `$app` must not inherit that.
                let scoped = ScopedHooks::new(
                    state.hooks.as_ref(),
                    HookContext::anonymous(&resolved.collection.name),
                );
                let record = records::create_with(
                    &client,
                    &resolved.collection,
                    &resolved.context,
                    input,
                    &extra,
                    &scoped,
                )
                .await
                .map_err(ApiError::from)?;
                let created = record
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ApiError::new(Error::internal("the created record has no id")))?
                    .to_string();
                // A concurrent callback for the same identity may have linked first. The
                // link is the identity, so the id it resolves to wins either way.
                crate::oauth2::link(
                    &client,
                    &collection,
                    &provider.name,
                    &user.provider_id,
                    &created,
                )
                .await
                .map_err(ApiError::new)?
            }
        };

    let record = records::get_one(
        &client,
        &resolved.collection,
        &resolved.context,
        &record_id,
        &FieldSelection::all(),
        &ExpandSpec::none(),
    )
    .await
    .map_err(ApiError::new)?
    .ok_or_else(|| ApiError::new(Error::internal("the linked record vanished")))?;

    let outcome = state
        .auth
        .issue_for_record(&resolved.collection, &record_id)
        .await
        .map_err(ApiError::new)?;

    Ok((StatusCode::OK, Json(auth_body(&outcome, record))).into_response())
}

/// Build the record input for a first OAuth2 sign-in.
///
/// The email and password come from the server — a random password, because the
/// account signs in through the provider — and `createData` may fill the rest,
/// except the fields that decide identity or credentials.
fn oauth2_create_input(
    collection: &Collection,
    user: &crate::oauth2::OAuthUser,
    create_data: Option<&Value>,
) -> Value {
    use lazypock_db::types::FieldType;

    let email_field = collection
        .fields
        .iter()
        .find(|field| field.field_type == FieldType::Email)
        .map(|field| field.name.clone())
        .unwrap_or_else(|| "email".to_string());
    let password_field = collection
        .fields
        .iter()
        .find(|field| field.field_type == FieldType::Password)
        .map(|field| field.name.clone())
        .unwrap_or_else(|| "password_hash".to_string());

    let mut object = serde_json::Map::new();
    if let Some(email) = &user.email {
        object.insert(email_field.clone(), json!(email));
    }
    object.insert(password_field.clone(), json!(random_password()));

    // The provider verified the address, so the account starts verified.
    if collection
        .fields
        .iter()
        .any(|field| field.name == "verified")
    {
        object.insert("verified".to_string(), json!(true));
    }
    if let Some(name) = &user.name {
        if collection.fields.iter().any(|field| field.name == "name") {
            object.insert("name".to_string(), json!(name));
        }
    }
    if let Some(picture) = &user.picture {
        if collection.fields.iter().any(|field| field.name == "avatar") {
            object.insert("avatar".to_string(), json!(picture));
        }
    }

    if let Some(Value::Object(extra)) = create_data {
        for (key, value) in extra {
            let allowed = !is_protected_create_field(key)
                && key != &email_field
                && key != &password_field
                && collection.fields.iter().any(|field| field.name == *key);
            if allowed {
                object.insert(key.clone(), value.clone());
            }
        }
    }

    Value::Object(object)
}

/// Fields a client may never set through `createData`.
fn is_protected_create_field(key: &str) -> bool {
    matches!(
        key,
        "id" | "created"
            | "updated"
            | "collectionId"
            | "collectionName"
            | "expand"
            | "password"
            | "passwordConfirm"
            | "tokenKey"
            | "verified"
            | "emailVisibility"
    )
}

fn random_password() -> String {
    let mut bytes = [0u8; 24];
    let _ = getrandom::fill(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// `GET /api/oauth2-redirect` — the popup bridge.
///
/// The provider sends the popup here. This page does not touch a record: it posts
/// the code back to the origin that started the flow (looked up from the state,
/// which is single-use) and closes itself. The `postMessage` target is never
/// taken from the request.
pub async fn oauth2_redirect(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let state_param = params.get("state").cloned().unwrap_or_default();

    let flow = match state.db.client().await {
        Ok(client) => crate::oauth2::take_flow(&client, &state_param)
            .await
            .unwrap_or(None),
        Err(_) => None,
    };
    let origin = flow
        .as_ref()
        .and_then(|flow| flow.get("app_origin"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if flow.is_none() {
        return oauth2_bridge(
            &origin,
            json!({
                "type": "lazypock:oauth2:error",
                "message": "This sign-in link has expired or was already used.",
            }),
        );
    }

    if let Some(error) = params.get("error").filter(|error| !error.is_empty()) {
        return oauth2_bridge(
            &origin,
            json!({
                "type": "lazypock:oauth2:error",
                "message": format!("The provider rejected the sign-in ({error})."),
            }),
        );
    }

    match params.get("code").filter(|code| !code.is_empty()) {
        Some(code) => oauth2_bridge(
            &origin,
            json!({ "type": "lazypock:oauth2", "code": code, "state": state_param }),
        ),
        None => oauth2_bridge(
            &origin,
            json!({
                "type": "lazypock:oauth2:error",
                "message": "The provider returned no authorization code.",
            }),
        ),
    }
}

/// Render the popup bridge page.
///
/// The payload is embedded as JSON in a `type="application/json"` element rather
/// than interpolated into the script, so nothing a provider or a code contains
/// can become script. A missing origin is refused rather than broadcast to `*`:
/// the code is a credential, and `*` would hand it to whatever opened the popup.
fn oauth2_bridge(origin: &str, payload: Value) -> Response {
    if origin.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Html(
                "<p>This sign-in could not be completed: the app origin is unknown.</p>"
                    .to_string(),
            ),
        )
            .into_response();
    }

    let payload = serde_json::to_string(&payload)
        .unwrap_or_default()
        .replace("</", "<\\/");
    let target = serde_json::to_string(origin).unwrap_or_default();
    let html = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Signing in…</title>\
         <script id=\"payload\" type=\"application/json\">{payload}</script>\
         <script>\
         (function(){{\
           var p = JSON.parse(document.getElementById('payload').textContent);\
           if (window.opener) {{ window.opener.postMessage(p, {target}); }}\
           window.close();\
         }})();\
         </script>"
    );

    (StatusCode::OK, Html(html)).into_response()
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
/// The superuser making the request, and who they are.
///
/// The id is carried so an admin action can name its actor in the audit trail;
/// without it the log would say what changed and never who changed it.
#[derive(Debug, Clone)]
pub struct Superuser {
    /// The acting superuser's record id in `_superusers`.
    pub id: String,
}

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
        Ok(Superuser {
            id: principal.user_id().unwrap_or_default().to_string(),
        })
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
