//! The lazypock axum routers.

use axum::{
    middleware,
    routing::{get, patch, post},
    Json, Router,
};
use lazypock_core::error::Error;
use serde_json::{json, Value};

use crate::auth;
use crate::batch;
use crate::error::ApiError;
use crate::files;
use crate::presets;
use crate::realtime;
use crate::records;
use crate::state::AppState;

/// The full application router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/batch", post(batch::batch))
        // The public half of the token signing key, for other services.
        .route("/api/jwks", get(auth::jwks))
        .route("/api/realtime", get(realtime::realtime))
        // The body limit bounds what is buffered before validation can reject it.
        .route("/api/files", post(files::upload).layer(files::body_limit()))
        .route(
            "/api/files/{collection}/{record}/{filename}",
            get(files::serve).delete(files::delete),
        )
        .route(
            "/api/files/{collection}/{record}/{filename}/thumbs/{preset}",
            get(files::serve_thumbnail),
        )
        // Reading the available sizes is public: a client needs them to build a
        // thumbnail URL. Changing them is a superuser action, enforced in the
        // handlers.
        .route(
            "/api/image-presets",
            get(presets::list).post(presets::create),
        )
        .route(
            "/api/image-presets/{name}",
            patch(presets::update).delete(presets::delete),
        )
        .route("/.well-known/jwks.json", get(auth::jwks))
        // `_superusers` is handled inside the same handler, so there is no route
        // ordering to get wrong.
        .route(
            "/api/collections/{collection}/auth-with-password",
            post(auth::auth_with_password),
        )
        .route(
            "/api/collections/{collection}/auth-refresh",
            post(auth::auth_refresh),
        )
        .route(
            "/api/collections/{collection}/auth-methods",
            get(auth::auth_methods),
        )
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
        // Authentication wraps every route, so any handler can read the caller.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::authenticate,
        ))
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
