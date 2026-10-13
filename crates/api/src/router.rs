//! The lazypock axum routers.

use axum::{
    routing::{get, post},
    Json, Router,
};
use lazypock_core::error::Error;
use serde_json::{json, Value};

use crate::batch;
use crate::error::ApiError;
use crate::records;
use crate::state::AppState;

/// The full application router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/batch", post(batch::batch))
        .route(
            "/api/collections/{collection}/records",
            get(records::list_records).post(records::create_record),
        )
        .route(
            "/api/collections/{collection}/records/{id}",
            get(records::view_record)
                .patch(records::update_record)
                .delete(records::delete_record),
        )
        .fallback(not_found)
        .with_state(state)
}

/// Routes that need no database, useful on their own for probes and tests.
pub fn health_router() -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/batch", post(batch_requires_state))
        .fallback(not_found)
}

/// A batch without application state cannot do anything useful; saying so is
/// better than a confusing 404.
async fn batch_requires_state() -> ApiError {
    ApiError::new(Error::unsupported(
        "the batch endpoint needs the database-backed router",
    ))
}

/// `GET /api/health` — PocketBase-compatible health probe.
///
/// Deliberately does not touch the database: liveness should not flap when
/// Postgres is briefly unavailable. Readiness is a separate concern.
async fn health() -> Json<Value> {
    Json(json!({
        "code": 200,
        "message": "OK",
        "data": {},
    }))
}

/// Any unmatched route returns the standard error envelope.
async fn not_found() -> ApiError {
    ApiError::new(Error::not_found("The requested resource was not found."))
}
