//! Sign-in, token refresh, sign-out and "who am I".
//!
//! Responses to a sign-in or refresh: `{ token, refreshToken, expiresIn, record }`.
//! The superuser signs in through the reserved collection name `_superusers`.

use crate::error::{ApiError, ApiResult, too_many_requests};
use crate::extract::{ClientInfo, Principal};
use crate::login_guard;
use crate::state::AppState;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use lazypock_auth::AuthOutcome;
use lazypock_auth::superuser::COLLECTION as SUPERUSERS;
use lazypock_core::Error;
use lazypock_db::{Collection, RequestContext, records};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub fn config(r: Router<AppState>) -> Router<AppState> {
    r.route(
        "/api/collections/{collection}/auth-with-password",
        post(auth_with_password),
    )
    .route(
        "/api/collections/{collection}/auth-refresh",
        post(auth_refresh),
    )
    .route("/api/collections/{collection}/logout", post(logout))
    .route("/api/me", get(me))
    .route("/.well-known/jwks.json", get(jwks))
}

#[derive(Deserialize)]
struct PasswordAuth {
    identity: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefreshBody {
    refresh_token: String,
}

fn auth_collection(state: &AppState, name: &str) -> ApiResult<Arc<Collection>> {
    state
        .schemas
        .get(name)
        .filter(|c| c.ty.is_auth())
        .ok_or_else(|| ApiError(Error::NotFound(format!("auth collection {name}"))))
}

/// The record that goes back to the client: read as the owner so it does not depend on
/// `viewRule`; credentials are not selectable columns, so they cannot appear.
async fn record_json(state: &AppState, outcome: &AuthOutcome) -> ApiResult<Value> {
    if outcome.collection_id.is_empty() {
        let claims = state.auth.verify(&outcome.access_token)?;
        return Ok(
            json!({ "id": outcome.user_id, "email": claims.email, "collectionName": SUPERUSERS }),
        );
    }
    let snapshot = state.schemas.cache().snapshot();
    let c = snapshot.get_by_id(&outcome.collection_id).ok_or_else(|| {
        ApiError(Error::Unauthorized(
            "the collection no longer exists".into(),
        ))
    })?;
    let mut record = records::view_as(&state.db, c, &RequestContext::owner(), &outcome.user_id)
        .await?
        .ok_or_else(|| ApiError(Error::Unauthorized("the account no longer exists".into())))?;
    if let Some(o) = record.as_object_mut() {
        o.insert("collectionId".into(), json!(c.id));
        o.insert("collectionName".into(), json!(c.name.as_str()));
    }
    Ok(record)
}

async fn body_for(state: &AppState, outcome: AuthOutcome) -> ApiResult<Response> {
    let record = record_json(state, &outcome).await?;
    Ok(Json(json!({
        "token": outcome.access_token,
        "refreshToken": outcome.refresh_token,
        "expiresIn": outcome.expires_in,
        "record": record,
    }))
    .into_response())
}

async fn auth_with_password(
    State(state): State<AppState>,
    ClientInfo(meta): ClientInfo,
    Path(collection): Path<String>,
    body: Result<Json<PasswordAuth>, JsonRejection>,
) -> ApiResult<Response> {
    let Json(req) = body?;

    let key = login_guard::key(&collection, &req.identity);
    if let Err(seconds) = state.login_guard.check(&key) {
        tracing::warn!(collection = %collection, "sign-in refused: the account is locked after failed attempts");
        return Ok(too_many_requests(
            seconds,
            "Too many failed sign-ins. Try again later.",
        ));
    }

    let outcome = if collection == SUPERUSERS {
        state
            .auth
            .authenticate_superuser(&req.identity, &req.password, &meta)
            .await
    } else {
        let c = auth_collection(&state, &collection)?;
        state
            .auth
            .authenticate_record(&c, &req.identity, &req.password, &meta)
            .await
    };
    match outcome {
        Ok(outcome) => {
            state.login_guard.success(&key);
            body_for(&state, outcome).await
        }
        Err(e @ Error::Unauthorized(_)) => {
            state.login_guard.failure(&key);
            Err(e.into())
        }
        Err(e) => Err(e.into()),
    }
}

async fn auth_refresh(
    State(state): State<AppState>,
    ClientInfo(meta): ClientInfo,
    Path(collection): Path<String>,
    body: Result<Json<RefreshBody>, JsonRejection>,
) -> ApiResult<Response> {
    let Json(req) = body?;
    let snapshot = state.schemas.cache().snapshot();
    let outcome = state
        .auth
        .refresh(&req.refresh_token, &snapshot, Some(&collection), &meta)
        .await?;
    body_for(&state, outcome).await
}

async fn logout(
    State(state): State<AppState>,
    Path(_collection): Path<String>,
    body: Result<Json<RefreshBody>, JsonRejection>,
) -> ApiResult<StatusCode> {
    let Json(req) = body?;
    state.auth.logout(&req.refresh_token).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(State(state): State<AppState>, principal: Principal) -> ApiResult<Json<Value>> {
    let Principal::Authenticated(claims) = &principal else {
        return Err(ApiError(Error::Unauthorized(
            "authentication required".into(),
        )));
    };
    let snapshot = state.schemas.cache().snapshot();
    principal.context(&state, &snapshot).await?; // account exists, token not revoked
    if claims.typ.is_superuser() {
        return Ok(Json(
            json!({ "id": claims.sub, "email": claims.email, "collectionName": SUPERUSERS }),
        ));
    }
    let c = snapshot.get_by_id(&claims.col).ok_or_else(|| {
        ApiError(Error::Unauthorized(
            "the collection no longer exists".into(),
        ))
    })?;
    let mut record = records::view_as(&state.db, c, &RequestContext::owner(), &claims.sub)
        .await?
        .ok_or_else(|| ApiError(Error::Unauthorized("the account no longer exists".into())))?;
    if let Some(o) = record.as_object_mut() {
        o.insert("collectionId".into(), json!(c.id));
        o.insert("collectionName".into(), json!(c.name.as_str()));
    }
    Ok(Json(record))
}

async fn jwks(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(state.auth.jwks()?))
}
