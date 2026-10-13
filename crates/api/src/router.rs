//! The lazypock axum router.

use std::sync::Arc;

use axum::{routing::get, Json, Router};
use lazypock_core::{config::Config, error::Error};
use serde_json::{json, Value};

use crate::error::ApiError;

/// Shared application state handed to every handler.
#[derive(Clone)]
pub struct AppState {
    /// Resolved runtime configuration.
    pub config: Arc<Config>,
}

impl AppState {
    /// Wrap a [`Config`] into shared state.
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

/// Build the application router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .fallback(not_found)
        .with_state(state)
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
