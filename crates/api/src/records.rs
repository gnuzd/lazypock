//! Record endpoints.
//!
//! Follows PocketBase's contract: a list response is
//! `{page, perPage, totalItems, totalPages, items}`, a single record *is* the
//! record document, delete is `204`, and errors are `{status, message, data}`.
//!
//! Authentication and access rules are not wired in yet, so every request is
//! treated as anonymous for now (Track E).

use std::sync::Arc;
use std::time::SystemTime;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use lazypock_db::tokio_postgres::types::ToSql;
use serde::Deserialize;
use serde_json::{json, Value};

use lazypock_core::error::Error;
use lazypock_db::collections::Collection;
use lazypock_db::pool::db_error;
use lazypock_db::query::compile::{AuthContext, FilterContext};
use lazypock_db::query::filter;
use lazypock_db::query::pagination::{Cursor, Page};
use lazypock_db::query::projection::{ExpandSpec, FieldSelection};
use lazypock_db::query::read::{self, BuiltQuery, ReadQuery};
use lazypock_db::query::sort::{self, SortSpec};
use lazypock_db::records;
use lazypock_db::schema::Schema;

use crate::error::ApiError;
use crate::state::AppState;

/// Query parameters accepted by the record endpoints.
///
/// `fields`, `expand`, `filter`, `sort`, `page`, `perPage` and `skipTotal`
/// mirror PocketBase; `after` is the keyset cursor.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    /// Filter expression.
    pub filter: Option<String>,
    /// Sort keys, e.g. `-created,title`.
    pub sort: Option<String>,
    /// Projection.
    pub fields: Option<String>,
    /// Relations to expand.
    pub expand: Option<String>,
    /// 1-based page number.
    pub page: Option<u32>,
    /// Rows per page.
    pub per_page: Option<u32>,
    /// Skip the total count.
    pub skip_total: Option<bool>,
    /// Keyset cursor from a previous response.
    pub after: Option<String>,
}

pub(crate) struct Resolved {
    pub collection: Collection,
    pub context: FilterContext,
}

pub(crate) fn resolve(state: &AppState, name: &str) -> Result<(Arc<Schema>, Resolved), ApiError> {
    let schema = state.schema.snapshot();
    let collection = schema
        .get(name)
        .cloned()
        .ok_or_else(|| ApiError::new(Error::not_found(format!("no collection named {name:?}"))))?;
    let context =
        FilterContext::for_collection(&collection, schema.collections()).map_err(ApiError::new)?;
    Ok((
        schema,
        Resolved {
            collection,
            context,
        },
    ))
}

fn build_read_query(params: &ListParams, context: &FilterContext) -> Result<ReadQuery, ApiError> {
    let selection =
        FieldSelection::parse(params.fields.as_deref(), context).map_err(ApiError::new)?;
    let expand = ExpandSpec::parse(params.expand.as_deref(), context).map_err(ApiError::new)?;
    let filter = filter::parse_optional(params.filter.as_deref().unwrap_or_default())
        .map_err(ApiError::new)?;
    let sort = match params.sort.as_deref().map(str::trim) {
        Some(raw) if !raw.is_empty() => sort::parse(raw),
        _ => SortSpec::default_spec(),
    }
    .map_err(ApiError::new)?;
    let page = Page::from_params(
        params.page,
        params.per_page,
        params.skip_total.unwrap_or(false),
        params.after.as_deref(),
    )
    .map_err(ApiError::new)?;

    Ok(ReadQuery {
        selection,
        expand,
        filter,
        sort,
        page,
    })
}

/// `GET /api/collections/{collection}/records`
pub async fn list_records(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Response, ApiError> {
    let (_schema, resolved) = resolve(&state, &collection)?;
    // Track E replaces this with the request's claims.
    let auth = AuthContext::default();

    let read_query = build_read_query(&params, &resolved.context)?;
    let built = read::build_list(&read_query, &resolved.context, &auth).map_err(ApiError::new)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    let rows = run_list(&client, &built).await?;

    let total = match &read_query.page {
        Page::Offset(request) if !request.skip_total => {
            let count = read::build_count(read_query.filter.as_ref(), &resolved.context, &auth)
                .map_err(ApiError::new)?;
            Some(run_count(&client, &count).await?)
        }
        _ => None,
    };

    // Only a full page can be continued, so the client never makes a final
    // request that returns nothing.
    let next_cursor = match &read_query.page {
        Page::Keyset { .. } if rows.len() as i64 == read_query.page.limit() => rows
            .last()
            .map(|row| Cursor::from_time(row.created, row.id.clone()).map_err(ApiError::new))
            .transpose()?
            .map(|cursor| cursor.encode()),
        _ => None,
    };

    let items = rows.into_iter().map(|row| row.record).collect();
    Ok((
        StatusCode::OK,
        Json(list_body(&read_query.page, items, total, next_cursor)),
    )
        .into_response())
}

/// `GET /api/collections/{collection}/records/{id}`
pub async fn view_record(
    State(state): State<AppState>,
    Path((collection, id)): Path<(String, String)>,
    Query(params): Query<ListParams>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (_schema, resolved) = resolve(&state, &collection)?;

    let selection = FieldSelection::parse(params.fields.as_deref(), &resolved.context)
        .map_err(ApiError::new)?;
    let expand =
        ExpandSpec::parse(params.expand.as_deref(), &resolved.context).map_err(ApiError::new)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    let record = records::get_one(
        &client,
        &resolved.collection,
        &resolved.context,
        &id,
        &selection,
        &expand,
    )
    .await
    .map_err(ApiError::new)?
    .ok_or_else(|| {
        ApiError::new(Error::not_found(format!(
            "no record {id:?} in collection {collection:?}"
        )))
    })?;

    // A narrowed projection may not carry `updated`, in which case there is
    // nothing to validate a conditional request against.
    let etag = record_etag(&record);
    if let Some(etag) = &etag {
        if etag_matches(&headers, etag) {
            return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, etag.as_str())]).into_response());
        }
    }

    let mut response = (StatusCode::OK, Json(record)).into_response();
    if let Some(etag) = etag.and_then(|etag| HeaderValue::from_str(&etag).ok()) {
        response.headers_mut().insert(header::ETAG, etag);
    }
    Ok(response)
}

/// `POST /api/collections/{collection}/records`
pub async fn create_record(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = json_body(body)?;
    let (_schema, resolved) = resolve(&state, &collection)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    let record = records::create(&client, &resolved.collection, &resolved.context, body)
        .await
        .map_err(ApiError::from)?;

    Ok((StatusCode::CREATED, Json(record)).into_response())
}

/// `PATCH /api/collections/{collection}/records/{id}`
pub async fn update_record(
    State(state): State<AppState>,
    Path((collection, id)): Path<(String, String)>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = json_body(body)?;
    let (_schema, resolved) = resolve(&state, &collection)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    let record = records::update(&client, &resolved.collection, &resolved.context, &id, body)
        .await
        .map_err(ApiError::from)?;

    match record {
        Some(record) => Ok((StatusCode::OK, Json(record)).into_response()),
        None => Err(not_found(&collection, &id)),
    }
}

/// `DELETE /api/collections/{collection}/records/{id}`
pub async fn delete_record(
    State(state): State<AppState>,
    Path((collection, id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let (_schema, resolved) = resolve(&state, &collection)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    let removed = records::delete(&client, &resolved.collection, &id)
        .await
        .map_err(ApiError::from)?;

    if removed {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(not_found(&collection, &id))
    }
}

fn not_found(collection: &str, id: &str) -> ApiError {
    ApiError::new(Error::not_found(format!(
        "no record {id:?} in collection {collection:?}"
    )))
}

fn json_body(body: Result<Json<Value>, JsonRejection>) -> Result<Json<Value>, ApiError> {
    body.map_err(|rejection| {
        ApiError::new(Error::invalid(format!(
            "invalid JSON body: {}",
            rejection.body_text()
        )))
    })
}

/// The PocketBase list envelope, or the keyset variant when cursor paging.
fn list_body(
    page: &Page,
    items: Vec<Value>,
    total: Option<i64>,
    next_cursor: Option<String>,
) -> Value {
    match page {
        Page::Offset(request) => {
            // -1 is PocketBase's marker for "not counted".
            let total_items = total.unwrap_or(-1);
            let total_pages = if total_items < 0 {
                -1
            } else {
                request.total_pages(total_items)
            };
            json!({
                "page": request.page,
                "perPage": request.per_page,
                "totalItems": total_items,
                "totalPages": total_pages,
                "items": items,
            })
        }
        Page::Keyset { limit, .. } => json!({
            "perPage": limit,
            "items": items,
            "nextCursor": next_cursor,
        }),
    }
}

struct ListRow {
    record: Value,
    created: SystemTime,
    id: String,
}

async fn run_list(
    client: &lazypock_db::tokio_postgres::Client,
    built: &BuiltQuery,
) -> Result<Vec<ListRow>, ApiError> {
    let rows = client
        .query(&built.sql, &bind(&built.params))
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;
    Ok(rows
        .iter()
        .map(|row| ListRow {
            record: row.get("record"),
            created: row.get("keyset_created"),
            id: row.get("keyset_id"),
        })
        .collect())
}

async fn run_count(
    client: &lazypock_db::tokio_postgres::Client,
    built: &BuiltQuery,
) -> Result<i64, ApiError> {
    let row = client
        .query_one(&built.sql, &bind(&built.params))
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;
    Ok(row.get(0))
}

fn bind(params: &[String]) -> Vec<&(dyn ToSql + Sync)> {
    params
        .iter()
        .map(|param| param as &(dyn ToSql + Sync))
        .collect()
}

/// A strong entity tag derived from the record's identity and version.
fn record_etag(record: &Value) -> Option<String> {
    let id = record.get("id")?.as_str()?;
    let updated = record.get("updated")?.as_str()?;
    Some(format!("\"{id}:{updated}\""))
}

fn etag_matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .any(|candidate| candidate.trim() == etag || candidate.trim() == "*")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_list_body_matches_the_pocketbase_shape() {
        let page = Page::from_params(Some(2), Some(3), false, None).unwrap();
        let body = list_body(&page, vec![json!({"id": "a"})], Some(7), None);
        assert_eq!(body["page"], 2);
        assert_eq!(body["perPage"], 3);
        assert_eq!(body["totalItems"], 7);
        assert_eq!(body["totalPages"], 3);
        assert_eq!(body["items"][0]["id"], "a");
    }

    #[test]
    fn skipped_totals_use_the_pocketbase_sentinel() {
        let page = Page::from_params(Some(1), Some(3), true, None).unwrap();
        let body = list_body(&page, vec![], None, None);
        assert_eq!(body["totalItems"], -1);
        assert_eq!(body["totalPages"], -1);
    }

    #[test]
    fn keyset_list_body_reports_a_cursor() {
        let page = Page::from_params(None, Some(2), false, Some("")).unwrap();
        let body = list_body(&page, vec![], None, Some("token".into()));
        assert_eq!(body["perPage"], 2);
        assert_eq!(body["nextCursor"], "token");
        assert!(body.get("totalItems").is_none());
    }

    #[test]
    fn etags_are_derived_from_identity_and_version() {
        let record = json!({"id": "abc", "updated": "2026-01-02T03:04:05Z"});
        let etag = record_etag(&record).expect("etag");
        assert_eq!(etag, "\"abc:2026-01-02T03:04:05Z\"");

        // Without a version there is nothing to validate against.
        assert!(record_etag(&json!({"id": "abc"})).is_none());
        assert!(record_etag(&json!({"updated": "x"})).is_none());
    }

    #[test]
    fn conditional_requests_match_lists_and_wildcards() {
        let mut headers = HeaderMap::new();
        assert!(!etag_matches(&headers, "\"a:1\""));

        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static("\"a:1\""));
        assert!(etag_matches(&headers, "\"a:1\""));
        assert!(!etag_matches(&headers, "\"a:2\""));

        headers.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_static("\"x\", \"a:1\""),
        );
        assert!(etag_matches(&headers, "\"a:1\""));

        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static("*"));
        assert!(etag_matches(&headers, "\"anything\""));
    }

    #[test]
    fn list_parameters_parse_from_camel_case_query_strings() {
        let params: ListParams = serde_urlencoded::from_str(
            "filter=title%3D%22a%22&sort=-created&perPage=10&skipTotal=true&fields=id&expand=author&page=2&after=abc",
        )
        .expect("query");
        assert_eq!(params.filter.as_deref(), Some("title=\"a\""));
        assert_eq!(params.sort.as_deref(), Some("-created"));
        assert_eq!(params.per_page, Some(10));
        assert_eq!(params.skip_total, Some(true));
        assert_eq!(params.fields.as_deref(), Some("id"));
        assert_eq!(params.expand.as_deref(), Some("author"));
        assert_eq!(params.page, Some(2));
        assert_eq!(params.after.as_deref(), Some("abc"));
    }
}
