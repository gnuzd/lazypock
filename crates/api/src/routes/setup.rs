//! First-run setup: the studio creates the first superuser here.
//!
//! `GET /api/setup` says whether setup is still possible; `POST` creates the superuser
//! and signs them in. Once a superuser exists the endpoint refuses (409), so it closes
//! itself on the first successful call.
//!
//! Two setups racing each other: `ensure_superuser` inserts under a count check, and the
//! loser sees `false` and gets the same 409.

use crate::error::{ApiError, ApiResult};
use crate::extract::ClientInfo;
use crate::state::AppState;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use lazypock_auth::superuser;
use lazypock_core::Error;
use serde::Deserialize;
use serde_json::{Value, json};

pub fn config(r: Router<AppState>) -> Router<AppState> {
    r.route("/api/setup", get(status).post(create))
}

async fn status(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let conn = state.db.conn().await?;
    Ok(Json(
        json!({ "required": superuser::count(&**conn).await? == 0 }),
    ))
}

#[derive(Deserialize)]
struct Setup {
    email: String,
    password: String,
}

async fn create(
    State(state): State<AppState>,
    ClientInfo(meta): ClientInfo,
    body: Result<Json<Setup>, JsonRejection>,
) -> ApiResult<Response> {
    let Json(req) = body?;
    if !state
        .auth
        .ensure_superuser(&req.email, &req.password)
        .await?
    {
        return Err(ApiError(Error::Conflict(
            "a superuser already exists".into(),
        )));
    }
    let outcome = state
        .auth
        .authenticate_superuser(&req.email, &req.password, &meta)
        .await?;
    let claims = state.auth.verify(&outcome.access_token)?;
    tracing::info!("first superuser created");
    Ok(Json(json!({
        "token": outcome.access_token,
        "refreshToken": outcome.refresh_token,
        "expiresIn": outcome.expires_in,
        "record": { "id": outcome.user_id, "email": claims.email, "collectionName": superuser::COLLECTION },
    }))
    .into_response())
}
