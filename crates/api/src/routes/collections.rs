//! Managing collections, fields and rules. **Superuser only.**
//!
//! Every change goes through `state.schemas` (`SchemaStore`), which reloads this node's
//! cache right after the write: the next request already sees the new collection.
//! Other nodes follow through NOTIFY.

use crate::error::{ApiError, ApiResult};
use crate::extract::SuperuserOnly;
use crate::state::AppState;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use lazypock_core::Error;
use lazypock_db::{Collection, Field, Ident, Rules};
use serde_json::{Value, json};

pub fn config(r: Router<AppState>) -> Router<AppState> {
    r.route("/api/collections", get(list).post(create))
        .route(
            "/api/collections/{name}",
            get(show).patch(update).delete(remove),
        )
        .route("/api/collections/{name}/fields", post(add_field))
        .route(
            "/api/collections/{name}/fields/{field}",
            delete(drop_field).patch(rename_field),
        )
}

fn ident(s: &str) -> ApiResult<Ident> {
    Ok(Ident::new(s)?)
}

fn parse<T: serde::de::DeserializeOwned>(v: Value, what: &str) -> ApiResult<T> {
    // serde's message names the offending field; it is ours, not a driver's.
    serde_json::from_value(v)
        .map_err(|e| ApiError(Error::Validation(format!("invalid {what}: {e}"))))
}

fn body(b: Result<Json<Value>, JsonRejection>) -> ApiResult<Value> {
    Ok(b?.0)
}

fn current(state: &AppState, name: &str) -> ApiResult<Json<Value>> {
    let c = state
        .schemas
        .get(name)
        .ok_or_else(|| ApiError(Error::NotFound(format!("collection {name}"))))?;
    Ok(Json(
        serde_json::to_value(&*c).map_err(|e| Error::Internal(e.to_string()))?,
    ))
}

async fn list(_: SuperuserOnly, State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let snapshot = state.schemas.cache().snapshot();
    let mut items: Vec<&Collection> = snapshot.iter().map(|c| &**c).collect();
    items.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
    let total = items.len();
    Ok(Json(
        json!({ "page": 1, "perPage": total, "totalItems": total, "items": items }),
    ))
}

async fn show(
    _: SuperuserOnly,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Value>> {
    current(&state, &name)
}

async fn create(
    _: SuperuserOnly,
    State(state): State<AppState>,
    b: Result<Json<Value>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Collection>)> {
    let collection: Collection = parse(body(b)?, "collection")?;
    Ok((
        StatusCode::CREATED,
        Json(state.schemas.create(&collection).await?),
    ))
}

/// `{ "name": "...", "rules": { "listRule": ..., ... } }`, either or both.
async fn update(
    _: SuperuserOnly,
    State(state): State<AppState>,
    Path(name): Path<String>,
    b: Result<Json<Value>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let body = body(b)?;
    let (new_name, rules) = (body.get("name"), body.get("rules"));
    if new_name.is_none() && rules.is_none() {
        return Err(ApiError(Error::BadRequest(
            "send `name` and/or `rules`".into(),
        )));
    }
    let from = ident(&name)?;
    if let Some(rules) = rules {
        let rules: Rules = parse(rules.clone(), "rules")?;
        state.schemas.set_rules(&from, &rules).await?;
    }
    let mut now = name;
    if let Some(n) = new_name {
        let to = ident(
            n.as_str()
                .ok_or_else(|| ApiError(Error::Validation("name must be a string".into())))?,
        )?;
        state.schemas.rename(&from, &to).await?;
        now = to.to_string();
    }
    current(&state, &now)
}

async fn remove(
    _: SuperuserOnly,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    state.schemas.delete(&ident(&name)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add_field(
    _: SuperuserOnly,
    State(state): State<AppState>,
    Path(name): Path<String>,
    b: Result<Json<Value>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Field>)> {
    let field: Field = parse(body(b)?, "field")?;
    Ok((
        StatusCode::CREATED,
        Json(state.schemas.add_field(&ident(&name)?, &field).await?),
    ))
}

async fn drop_field(
    _: SuperuserOnly,
    State(state): State<AppState>,
    Path((name, field)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    state
        .schemas
        .drop_field(&ident(&name)?, &ident(&field)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `{ "name": "newName" }`
async fn rename_field(
    _: SuperuserOnly,
    State(state): State<AppState>,
    Path((name, field)): Path<(String, String)>,
    b: Result<Json<Value>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let body = body(b)?;
    let to = ident(
        body.get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError(Error::Validation("name is required".into())))?,
    )?;
    state
        .schemas
        .rename_field(&ident(&name)?, &ident(&field)?, &to)
        .await?;
    current(&state, &name)
}
