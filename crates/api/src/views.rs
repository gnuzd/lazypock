//! The view-builder previews.
//!
//! Two superuser-only endpoints, both answering "what would this view be, and
//! what would come out of it" without creating anything:
//!
//! * `POST /api/collections/meta/dry-run-view` takes the SQL somebody typed in
//!   the view editor.
//! * `POST /api/collections/meta/preview-view-builder` takes the no-code spec and
//!   generates the SQL the builder would produce.
//!
//! Neither writes: the statement is run inside a read-only transaction, so a
//! preview cannot become a way to make something happen. The generated SQL is
//! returned to the caller, both because the editor shows it and because it is the
//! thing a person may want to keep.
//!
//! Errors keep the `<what>. Raw error: <why>` shape, because the Studio strips
//! that prefix and shows the rest — so the reason has to be in the right place to
//! survive being read.

use axum::extract::State;
use axum::Json;
use lazypock_core::error::Error;
use lazypock_db::view_builder;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::Superuser;
use crate::error::ApiError;
use crate::sql;
use crate::state::AppState;

/// How many sample rows a preview returns.
const SAMPLE_ROWS: i64 = 5;

/// What `POST /api/collections/meta/dry-run-view` sends.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DryRun {
    /// The view's SQL.
    pub query: String,
}

/// What `POST /api/collections/meta/preview-view-builder` sends.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preview {
    /// The no-code spec.
    pub view_builder: Value,
}

/// `POST /api/collections/meta/dry-run-view` — describe a hand-written view query.
pub async fn dry_run_view(
    State(state): State<AppState>,
    _: Superuser,
    Json(body): Json<DryRun>,
) -> Result<Json<Value>, ApiError> {
    let statement = sql::trim_statement(&body.query);
    sql::ensure_read_only(&statement).map_err(|err| reason("Invalid view query", err))?;

    let (columns, sample) = sql::describe(&state, &statement, SAMPLE_ROWS)
        .await
        .map_err(|err| reason("Invalid view query", err))?;

    Ok(Json(json!({
        "fields": fields(&columns),
        "sample": sample,
    })))
}

/// `POST /api/collections/meta/preview-view-builder` — build the SQL for a spec.
pub async fn preview_view_builder(
    State(state): State<AppState>,
    _: Superuser,
    Json(body): Json<Preview>,
) -> Result<Json<Value>, ApiError> {
    // The builder may only refer to collections that are installed, so a spec
    // naming one that does not exist is refused here rather than when it is saved.
    let collections = {
        let client = state.db.client().await.map_err(ApiError::new)?;
        lazypock_db::collections::list(&client)
            .await
            .map_err(ApiError::new)?
    };

    let query = view_builder::to_query(&body.view_builder, &collections).map_err(|err| {
        reason(
            "Invalid view builder spec",
            ApiError::new(Error::invalid(err.message().to_string())),
        )
    })?;

    let (columns, sample) = sql::describe(&state, &query, SAMPLE_ROWS)
        .await
        .map_err(|err| reason("Invalid view builder spec", err))?;

    Ok(Json(json!({
        "query": query,
        "fields": fields(&columns),
        "sample": sample,
    })))
}

/// The columns as the Studio reads them.
fn fields(columns: &[sql::Column]) -> Vec<Value> {
    columns
        .iter()
        .map(|column| json!({ "name": column.name, "type": column.type_name }))
        .collect()
}

/// Wrap a failure in the shape the Studio strips its prefix from.
fn reason(what: &str, err: ApiError) -> ApiError {
    ApiError::new(Error::invalid(format!(
        "{what}. Raw error: {}",
        err.error.message()
    )))
}
