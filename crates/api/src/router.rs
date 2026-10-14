//! The lazypock axum routers.

use axum::{
    middleware,
    routing::{delete, get, patch, post},
    Json, Router,
};
use lazypock_core::error::Error;
use serde_json::{json, Value};

use crate::auth;
use crate::backup;
use crate::batch;
use crate::collections as admin;
use crate::cors;
use crate::error::ApiError;
use crate::files;
use crate::logs;
use crate::presets;
use crate::realtime;
use crate::records;
use crate::settings;
use crate::state::AppState;
use crate::superusers;

/// The full application router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/batch", post(batch::batch))
        // The public half of the token signing key, for other services.
        .route("/api/jwks", get(auth::jwks))
        // Backup and restore. Superuser-only: an export is the whole database.
        .route("/api/export", get(backup::export_json))
        .route("/api/export/archive", get(backup::export))
        .route("/api/import/preflight", get(backup::preflight))
        .route(
            "/api/import",
            post(backup::import).layer(backup::body_limit()),
        )
        // PocketBase parity, and what a client calls to learn who it is.
        .route("/api/me", get(auth::me))
        // First-run setup. Public by necessity — a client calls these before anybody
        // can log in — and `setup` only works while no superuser exists.
        .route("/api/superusers/check", get(superusers::check))
        // Settings. Superuser-only: they hold SMTP credentials and storage keys.
        .route("/api/settings", get(settings::get).patch(settings::update))
        .route("/api/settings/refresh-cors", post(settings::refresh_cors))
        .route("/api/superusers/setup", post(superusers::setup))
        .route("/api/realtime", get(realtime::realtime))
        // The body limit bounds what is buffered before validation can reject it.
        .route(
            "/api/files",
            get(files::list)
                .post(files::upload)
                .layer(files::body_limit()),
        )
        .route(
            "/api/files/{collection}/{record}/{filename}",
            get(files::serve).delete(files::delete),
        )
        .route(
            "/api/files/{collection}/{record}/{filename}/thumbs/{preset}",
            get(files::serve_thumbnail),
        )
        .route("/api/files/sweep", post(files::sweep))
        // Request logs. Superuser-only: they carry IPs, paths and user agents.
        .route("/api/logs", get(logs::list).delete(logs::delete))
        .route("/api/logs/stats", get(logs::stats))
        .route("/api/logs/collections", get(logs::collections))
        .route(
            "/api/logs/retention",
            get(logs::get_retention).patch(logs::set_retention),
        )
        .route("/api/logs/{id}", get(logs::show))
        // Reading the available sizes is public: a client needs them to build a
        // thumbnail URL. Changing them is a superuser action, enforced in the
        // handlers.
        // Collection administration. Superuser-only throughout, and the reads
        // included: a collection's definition carries its access rules.
        .route("/api/collections", get(admin::list).post(admin::create))
        .route(
            "/api/collections/{name}",
            get(admin::get).patch(admin::update).delete(admin::delete),
        )
        .route("/api/collections/{name}/fields", post(admin::add_field))
        .route(
            "/api/collections/{name}/fields/{field}",
            delete(admin::delete_field),
        )
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
        // Outside authentication, so a refused request is still logged. Inside
        // CORS, so preflights are answered without a log write.
        .layer(middleware::from_fn_with_state(state.clone(), logs::record))
        // Outermost, and it has to be: a preflight carries no credentials, so it must
        // be answered before authentication runs, and a 401 still needs its CORS
        // headers or the browser reports an opaque network error instead of the
        // reason.
        //
        // It also has to come *after* the routes above. `Router::layer` wraps only
        // what is already registered, so a layer added at the top wraps nothing —
        // which is how the first version of this shipped with CORS implemented and
        // never applied.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            cors::apply,
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
