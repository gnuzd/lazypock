//! Records: list, view, create, update, delete.
//!
//! Access is decided by the collection's rules, enforced by Postgres (row-level
//! security) for the caller's role: `anon`, `authenticated`, or the owner for a superuser.
//! A record the caller may not see simply does not exist for them (404), whether it is
//! missing or forbidden.
//!
//! A create is judged by `createRule` alone; the record in the response is read back as
//! the owner, so a user can create what they are not allowed to list.

use crate::error::{ApiError, ApiResult};
use crate::extract::Principal;
use crate::state::AppState;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use lazypock_core::Error;
use lazypock_db::records::{self, ListParams};
use lazypock_db::{Collection, RequestContext};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

pub fn config(r: Router<AppState>) -> Router<AppState> {
    r.route(
        "/api/collections/{collection}/records",
        get(list).post(create),
    )
    .route(
        "/api/collections/{collection}/records/{id}",
        get(view).patch(update).delete(remove),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    filter: Option<String>,
    sort: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
}

/// The collection and the caller's database context for one request.
async fn scope(
    state: &AppState,
    principal: &Principal,
    name: &str,
) -> ApiResult<(Arc<Collection>, RequestContext)> {
    let snapshot = state.schemas.cache().snapshot();
    let collection = snapshot
        .get(name)
        .cloned()
        .ok_or_else(|| ApiError(Error::NotFound(format!("collection {name}"))))?;
    let ctx = principal.context(state, &snapshot).await?;
    Ok((collection, ctx))
}

fn record_not_found(id: &str) -> ApiError {
    ApiError(Error::NotFound(format!("record {id}")))
}

async fn list(
    State(state): State<AppState>,
    principal: Principal,
    Path(name): Path<String>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> ApiResult<Json<records::Page>> {
    let Query(q) = query?;
    let (c, ctx) = scope(&state, &principal, &name).await?;
    let page = records::list_as(
        &state.db,
        &c,
        &ctx,
        &ListParams {
            filter: q.filter.as_deref(),
            sort: q.sort.as_deref(),
            page: q.page.unwrap_or(1),
            per_page: q.per_page.unwrap_or(30),
        },
    )
    .await?;
    Ok(Json(page))
}

async fn view(
    State(state): State<AppState>,
    principal: Principal,
    Path((name, id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let (c, ctx) = scope(&state, &principal, &name).await?;
    records::view_as(&state.db, &c, &ctx, &id)
        .await?
        .map(Json)
        .ok_or_else(|| record_not_found(&id))
}

async fn create(
    State(state): State<AppState>,
    principal: Principal,
    Path(name): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> ApiResult<Response> {
    let Json(body) = body?;
    let (c, ctx) = scope(&state, &principal, &name).await?;
    let record = if c.ty.is_auth() {
        let prepared = state.auth.prepare_record_create(&body).await?;
        records::create_as(&state.db, &c, &ctx, &prepared.body, &prepared.extra()).await?
    } else {
        records::create_as(&state.db, &c, &ctx, &body, &[]).await?
    };
    Ok((StatusCode::CREATED, Json(record)).into_response())
}

async fn update(
    State(state): State<AppState>,
    principal: Principal,
    Path((name, id)): Path<(String, String)>,
    body: Result<Json<Value>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let Json(body) = body?;
    let (c, ctx) = scope(&state, &principal, &name).await?;
    let (updated, password_changed) = if c.ty.is_auth() {
        // Everyone but a superuser must prove they know the current password to change it.
        let prepared = state
            .auth
            .prepare_record_update(&c, &id, &body, !principal.is_superuser())
            .await?;
        let record =
            records::update_as(&state.db, &c, &ctx, &id, &prepared.body, &prepared.extra()).await?;
        (record, prepared.password_changed)
    } else {
        (
            records::update_as(&state.db, &c, &ctx, &id, &body, &[]).await?,
            false,
        )
    };
    let record = updated.ok_or_else(|| record_not_found(&id))?;
    if password_changed {
        // The new token_key already cuts off access tokens; this ends the refresh sessions too.
        state.auth.revoke_all(&c.id, &id).await?;
    }
    Ok(Json(record))
}

async fn remove(
    State(state): State<AppState>,
    principal: Principal,
    Path((name, id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let (c, ctx) = scope(&state, &principal, &name).await?;
    if !records::delete_as(&state.db, &c, &ctx, &id).await? {
        return Err(record_not_found(&id));
    }
    if c.ty.is_auth() {
        state.auth.revoke_all(&c.id, &id).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
