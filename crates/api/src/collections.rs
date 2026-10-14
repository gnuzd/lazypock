//! Collection administration.
//!
//! Every endpoint here is **superuser-only**, reads included. A collection's
//! definition carries its access rules, so listing them is not public
//! information; a client that needs to address a collection by name learns it from
//! the records API, which resolves names on its own.
//!
//! Changing a table's shape goes through the field endpoints rather than the
//! collection update, so the DDL that alters real tables stays in one place in the
//! data layer.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lazypock_core::error::Error;
use lazypock_db::collections::{self, Collection, CreateCollection, CreateField, UpdateCollection};
use serde::Deserialize;

use crate::auth::require_superuser;
use crate::error::ApiError;
use crate::state::AppState;

/// `GET /api/collections`
pub async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<Vec<Collection>>, ApiError> {
    require_superuser(&principal)?;
    let client = state.db.client().await.map_err(ApiError::new)?;
    let collections = collections::list(&client).await.map_err(ApiError::new)?;
    Ok(Json(collections))
}

/// `GET /api/collections/{name}`
pub async fn get(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
) -> Result<Json<Collection>, ApiError> {
    require_superuser(&principal)?;
    let client = state.db.client().await.map_err(ApiError::new)?;
    let collection = collections::find(&client, &name)
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| not_found(&name))?;
    Ok(Json(collection))
}

/// `POST /api/collections`
///
/// The body is the data layer's own input type, so the wire shape and the layer
/// that builds the DDL cannot drift apart.
pub async fn create(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<CreateCollection>,
) -> Result<Response, ApiError> {
    require_superuser(&principal)?;

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    let collection = collections::create(&mut client, body)
        .await
        .map_err(ApiError::new)?;

    tracing::info!(collection = %collection.name, "collection created");
    Ok((StatusCode::CREATED, Json(collection)).into_response())
}

/// `PATCH /api/collections/{name}`
pub async fn update(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Json(body): Json<UpdateCollection>,
) -> Result<Json<Collection>, ApiError> {
    require_superuser(&principal)?;

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    // Confirm it exists first, so the error is a 404 rather than whatever the
    // update happens to report.
    if collections::find(&client, &name)
        .await
        .map_err(ApiError::new)?
        .is_none()
    {
        return Err(not_found(&name));
    }

    let collection = collections::update(&mut client, &name, body)
        .await
        .map_err(ApiError::new)?;
    tracing::info!(collection = %collection.name, "collection updated");
    Ok(Json(collection))
}

/// Query for deleting a collection.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteQuery {
    /// Whether to drop the table and its rows.
    ///
    /// Defaults to false, and the field is `dropData` rather than something
    /// friendlier because the decision deserves an explicit word: dropping a
    /// collection without this leaves the data in place and only removes the
    /// definition.
    #[serde(default, rename = "dropData")]
    pub drop_data: Option<bool>,
}

/// `DELETE /api/collections/{name}`
pub async fn delete(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<Collection>, ApiError> {
    require_superuser(&principal)?;

    let drop_data = query.drop_data.unwrap_or(false);
    let mut client = state.db.client().await.map_err(ApiError::new)?;
    if collections::find(&client, &name)
        .await
        .map_err(ApiError::new)?
        .is_none()
    {
        return Err(not_found(&name));
    }

    let collection = collections::delete(&mut client, &name, drop_data)
        .await
        .map_err(ApiError::new)?;

    tracing::warn!(collection = %name, drop_data, "collection deleted");
    Ok(Json(collection))
}

/// `POST /api/collections/{name}/fields`
pub async fn add_field(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Json(body): Json<CreateField>,
) -> Result<Response, ApiError> {
    require_superuser(&principal)?;

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    let field = collections::add_field(&mut client, &name, body)
        .await
        .map_err(ApiError::new)?;

    tracing::info!(collection = %name, field = %field.name, "field added");
    Ok((StatusCode::CREATED, Json(field)).into_response())
}

/// `DELETE /api/collections/{name}/fields/{field}`
pub async fn delete_field(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((name, field)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    require_superuser(&principal)?;

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    collections::delete_field(&mut client, &name, &field)
        .await
        .map_err(ApiError::new)?;

    tracing::warn!(collection = %name, field = %field, "field deleted");
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The error for a collection that is not there.
///
/// One helper so every endpoint reports it the same way.
fn not_found(name: &str) -> ApiError {
    ApiError::new(Error::not_found(format!("no collection named {name:?}")))
}

use axum::Extension;

use crate::auth::Principal;
