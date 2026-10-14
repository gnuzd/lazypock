//! `POST /api/batch`: several record operations inside **one transaction**.
//!
//! The request shape matches PocketBase — `{"requests": [{"method", "url",
//! "body"}]}` — and the response is an array of `{"status", "body"}` in request
//! order.
//!
//! Semantics are all-or-nothing: if any sub-request fails, the transaction is
//! rolled back and the failure is returned with the index of the culprit. A
//! batch therefore cannot leave a half-applied change behind.
//!
//! Only record URLs are accepted. Collection DDL inside a batch would need a
//! nested transaction, which Postgres does not have, so it is refused loudly
//! rather than silently half-supported.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use lazypock_core::error::Error;
use lazypock_db::collections::Collection;
use lazypock_db::pool::db_error;
use lazypock_db::query::compile::FilterContext;
use lazypock_db::query::projection::{ExpandSpec, FieldSelection};
use lazypock_db::records;
use lazypock_db::runner::QueryRunner;
use lazypock_db::schema::Schema;

use crate::auth::Principal;
use crate::error::ApiError;
use crate::state::AppState;

/// Largest number of sub-requests accepted in one batch.
pub const MAX_BATCH_REQUESTS: usize = 50;

/// A batch envelope.
#[derive(Debug, Deserialize)]
pub struct BatchRequest {
    /// Operations, applied in order.
    pub requests: Vec<BatchItem>,
}

/// One operation.
#[derive(Debug, Deserialize)]
pub struct BatchItem {
    /// HTTP method: `GET`, `POST`, `PATCH` or `DELETE`.
    pub method: String,
    /// A record URL.
    pub url: String,
    /// Body for a create or update.
    #[serde(default)]
    pub body: Option<Value>,
}

/// A planned and validated operation.
struct Step {
    method: Method,
    collection: Collection,
    context: FilterContext,
    id: Option<String>,
    input: Option<Value>,
}

/// `POST /api/batch`
pub async fn batch(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<BatchRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = body.map_err(|rejection| {
        ApiError::new(Error::invalid(format!(
            "invalid JSON body: {}",
            rejection.body_text()
        )))
    })?;

    if request.requests.is_empty() {
        return Err(ApiError::new(Error::invalid(
            "a batch needs at least one request",
        )));
    }
    if request.requests.len() > MAX_BATCH_REQUESTS {
        return Err(ApiError::new(Error::invalid(format!(
            "a batch accepts at most {MAX_BATCH_REQUESTS} requests, got {}",
            request.requests.len()
        ))));
    }

    // Plan everything before writing anything, so a batch that cannot possibly
    // succeed fails without touching the database at all.
    let schema = state.schema.snapshot();
    let mut steps = Vec::with_capacity(request.requests.len());
    for item in &request.requests {
        steps.push(plan(&schema, item)?);
    }

    let mut pooled = state.db.client().await.map_err(ApiError::new)?;
    let transaction = pooled
        .transaction()
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    match run(&transaction, &steps, &principal).await {
        Ok(results) => {
            transaction
                .commit()
                .await
                .map_err(|err| ApiError::new(db_error(err)))?;
            Ok((StatusCode::OK, Json(Value::Array(results))).into_response())
        }
        Err((index, error)) => {
            let _ = transaction.rollback().await;
            tracing::debug!(index, "batch rolled back");
            Err(error.context(format!("batch request {index}")))
        }
    }
}

fn plan(schema: &Schema, item: &BatchItem) -> Result<Step, ApiError> {
    if item.url.contains('?') || item.url.contains('#') {
        return Err(ApiError::new(Error::unsupported(
            "batch URLs cannot carry query parameters",
        )));
    }

    let method = match item.method.to_ascii_uppercase().as_str() {
        "POST" => Method::POST,
        "PATCH" | "PUT" => Method::PATCH,
        "DELETE" => Method::DELETE,
        "GET" => Method::GET,
        other => {
            return Err(ApiError::new(Error::unsupported(format!(
                "`{other}` is not supported in a batch; use GET, POST, PATCH or DELETE"
            ))))
        }
    };

    let (name, id) = parse_record_url(&item.url).ok_or_else(|| {
        ApiError::new(Error::invalid(format!(
            "`{}` is not a record URL; a batch accepts \
             /api/collections/{{collection}}/records or .../records/{{id}}",
            item.url
        )))
    })?;

    let collection = schema
        .get(&name)
        .cloned()
        .ok_or_else(|| ApiError::new(Error::not_found(format!("no collection named {name:?}"))))?;
    let context =
        FilterContext::for_collection(&collection, schema.collections()).map_err(ApiError::new)?;

    match method {
        Method::POST if id.is_some() => {
            return Err(ApiError::new(Error::invalid(
                "POST takes the collection URL without a record id",
            )))
        }
        Method::PATCH | Method::DELETE | Method::GET if id.is_none() => {
            return Err(ApiError::new(Error::invalid(format!(
                "{method} needs a record id in the URL"
            ))))
        }
        _ => {}
    }

    let input = match method {
        Method::POST | Method::PATCH => {
            let body = item.body.clone().ok_or_else(|| {
                ApiError::new(Error::invalid(format!("{method} needs a request body")))
            })?;
            if !body.is_object() {
                return Err(ApiError::new(Error::invalid(
                    "the request body must be a JSON object of field values",
                )));
            }
            Some(body)
        }
        _ => None,
    };

    Ok(Step {
        method,
        collection,
        context,
        id,
        input,
    })
}

async fn run(
    runner: &dyn QueryRunner,
    steps: &[Step],
    principal: &Principal,
) -> Result<Vec<Value>, (usize, ApiError)> {
    let mut results = Vec::with_capacity(steps.len());
    for (index, step) in steps.iter().enumerate() {
        // Re-applied for every sub-request: a write hands the transaction back to
        // the owner so it can read the stored row back, so without this the next
        // one would run unrestricted.
        lazypock_db::rls::context::apply(runner, &principal.request_context())
            .await
            .map_err(|error| (index, ApiError::new(error)))?;

        let (status, body) = execute(runner, step)
            .await
            .map_err(|error| (index, error))?;
        results.push(json!({ "status": status, "body": body }));
    }
    Ok(results)
}

async fn execute(runner: &dyn QueryRunner, step: &Step) -> Result<(u16, Value), ApiError> {
    match step.method.as_str() {
        "POST" => {
            let input = step.input.clone().expect("planned");
            let extra = crate::auth::auth_columns(&step.collection, &input)?;
            let record =
                records::create_with(runner, &step.collection, &step.context, input, &extra)
                    .await
                    .map_err(ApiError::from)?;
            Ok((201, record))
        }
        "PATCH" => {
            let input = step.input.clone().expect("planned");
            let id = step.id.clone().expect("planned");
            let extra = crate::auth::auth_columns(&step.collection, &input)?;
            match records::update_with(runner, &step.collection, &step.context, &id, input, &extra)
                .await
                .map_err(ApiError::from)?
            {
                Some(record) => Ok((200, record)),
                None => Err(missing(&step.collection.name, &id)),
            }
        }
        "DELETE" => {
            let id = step.id.clone().expect("planned");
            let removed = records::delete(runner, &step.collection, &id)
                .await
                .map_err(ApiError::from)?;
            if removed {
                Ok((204, Value::Null))
            } else {
                Err(missing(&step.collection.name, &id))
            }
        }
        "GET" => {
            let id = step.id.clone().expect("planned");
            let record = records::get_one(
                runner,
                &step.collection,
                &step.context,
                &id,
                &FieldSelection::all(),
                &ExpandSpec::none(),
            )
            .await
            .map_err(ApiError::new)?;
            match record {
                Some(record) => Ok((200, record)),
                None => Err(missing(&step.collection.name, &id)),
            }
        }
        other => Err(ApiError::new(Error::unsupported(format!(
            "{other} is not supported in a batch"
        )))),
    }
}

fn missing(collection: &str, id: &str) -> ApiError {
    ApiError::new(Error::not_found(format!(
        "no record {id:?} in collection {collection:?}"
    )))
}

/// Parse `/api/collections/{collection}/records` or `.../records/{id}`.
fn parse_record_url(url: &str) -> Option<(String, Option<String>)> {
    let rest = url.strip_prefix("/api/collections/")?;
    let (collection, rest) = rest.split_once('/')?;
    if collection.is_empty() {
        return None;
    }

    match rest.strip_prefix("records")? {
        "" => Some((collection.to_string(), None)),
        rest => {
            let id = rest.strip_prefix('/')?;
            if id.is_empty() || id.contains('/') {
                return None;
            }
            Some((collection.to_string(), Some(id.to_string())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_urls_parse() {
        assert_eq!(
            parse_record_url("/api/collections/posts/records"),
            Some(("posts".into(), None))
        );
        assert_eq!(
            parse_record_url("/api/collections/posts/records/abcDEF123456789"),
            Some(("posts".into(), Some("abcDEF123456789".into())))
        );
    }

    #[test]
    fn non_record_urls_are_rejected() {
        for bad in [
            "/api/collections",
            "/api/collections/posts",
            "/api/collections/posts/recordsX",
            "/api/collections/posts/records/",
            "/api/collections/posts/records/a/b",
            "/api/collections//records",
            "/api/collections/posts/fields",
            "posts/records",
            "",
        ] {
            assert!(
                parse_record_url(bad).is_none(),
                "{bad:?} should be rejected"
            );
        }
    }
}
