//! The lazypock axum routers.

use axum::{
    middleware,
    routing::{delete, get, patch, post},
    Json, Router,
};
use lazypock_core::error::Error;
use serde_json::{json, Value};

use crate::audit;
use crate::auth;
use crate::backup;
use crate::batch;
use crate::collections as admin;
use crate::cors;
use crate::crons;
use crate::emails;
use crate::error::ApiError;
use crate::files;
use crate::logs;
use crate::presets;
use crate::rate_limit;
use crate::realtime;
use crate::records;
use crate::settings;
use crate::sql;
use crate::state::AppState;
use crate::studio;
use crate::superusers;
use crate::views;

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
        .route("/api/import/status", get(backup::import_status))
        .route("/api/import/rollback", post(backup::import_rollback))
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
        // Settings import/export. The export redacts credentials, and the import
        // merges rather than replaces, so a round trip cannot delete a secret.
        .route("/api/settings/export", get(settings::export))
        .route("/api/settings/import", post(settings::import))
        // The Studio's Files Storage page: show and test the active configuration. A
        // write is refused with the variables to change instead — storage is
        // environment-only (PLAN section 10), and a page that silently saved nothing
        // would be worse than one that says so.
        .route(
            "/api/settings/storage",
            get(settings::storage_get).patch(settings::storage_update),
        )
        .route("/api/settings/storage/test", post(settings::storage_test))
        .route(
            "/api/settings/api-keys",
            get(settings::list_api_keys).post(settings::generate_api_key),
        )
        .route(
            "/api/settings/api-keys/{id}",
            delete(settings::revoke_api_key),
        )
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
        // Direct upload: reserve an object, PUT it straight to the bucket, then ask the
        // server to verify it. A backend that cannot presign answers 501.
        .route("/api/files/presign", post(files::presign))
        .route("/api/files/{id}/complete", post(files::complete))
        // The id-addressed pair: what a client holding a file field's value (or the id
        // an upload returned) uses. A key is three segments, so this does not overlap
        // the routes above, and the query carries the variant (`?preset=`, `?thumb=`).
        .route(
            "/api/files/{id}",
            get(files::serve_by_id).delete(files::delete_by_id),
        )
        // The path-shaped variants of the same request, as the SDK's `thumbs`/
        // `variants` maps document them.
        .route(
            "/api/files/{id}/thumbs/{size}",
            get(files::serve_thumbnail_by_id),
        )
        .route(
            "/api/files/{id}/scale/{size}",
            get(files::serve_scale_by_id),
        )
        // Request logs. Superuser-only: they carry IPs, paths and user agents.
        .route("/api/logs", get(logs::list).delete(logs::delete))
        .route("/api/logs/stats", get(logs::stats))
        .route("/api/logs/collections", get(logs::collections))
        .route(
            "/api/logs/retention",
            get(logs::get_retention).patch(logs::set_retention),
        )
        .route("/api/logs/{id}", get(logs::show))
        // The admin audit trail. Superuser-only, like the request log and for the
        // same reason: it names who did what, and when.
        .route("/api/audit", get(audit::list))
        // Cron jobs. Superuser-only: running one executes SQL or an HTTP request.
        .route("/api/crons", get(crons::list).post(crons::create))
        .route("/api/crons/validate", post(crons::validate))
        .route(
            "/api/crons/{id}",
            get(crons::get)
                .patch(crons::update)
                .delete(crons::delete)
                .post(crons::run_now),
        )
        // The SQL console. Superuser-only, which the extractor enforces, and
        // read-only: the handler refuses anything the database would have to
        // write for, and a read-only transaction catches what the keyword check
        // cannot.
        .route("/api/sql/query", post(sql::query))
        // The view previews. Superuser-only: each one runs a statement, and the
        // generated SQL is what a view would then be created from. Registered
        // before the `/api/collections/{name}` routes; the static segment wins,
        // so `meta` is never read as a collection name.
        .route(
            "/api/collections/meta/dry-run-view",
            post(views::dry_run_view),
        )
        .route(
            "/api/collections/meta/preview-view-builder",
            post(views::preview_view_builder),
        )
        // Reading the available sizes is public: a client needs them to build a
        // thumbnail URL. Changing them is a superuser action, enforced in the
        // handlers.
        // Collection administration. Writes are superuser-only; the reads also
        // accept an API key, which is the only thing a codegen client needs and is
        // exactly what the reference implementation scoped a key to.
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
        // The OAuth2 popup bridge. The provider redirects the popup here; the page
        // posts the code back to the app origin the flow started from.
        .route("/api/oauth2-redirect", get(auth::oauth2_redirect))
        // `_superusers` is handled inside the same handler, so there is no route
        // ordering to get wrong.
        .route(
            "/api/collections/{collection}/auth-with-password",
            post(auth::auth_with_password),
        )
        .route(
            "/api/collections/{collection}/auth-with-oauth2",
            post(auth::auth_with_oauth2),
        )
        .route(
            "/api/collections/{collection}/auth-refresh",
            post(auth::auth_refresh),
        )
        .route(
            "/api/collections/{collection}/auth-methods",
            get(auth::auth_methods),
        )
        // The email flows. The request half is public by necessity — a client calls
        // it before it can log in — and always answers 204; the confirm half applies
        // the change the token names.
        .route(
            "/api/collections/{collection}/request-verification",
            post(emails::request_verification),
        )
        .route(
            "/api/collections/{collection}/confirm-verification",
            post(emails::confirm_verification),
        )
        .route(
            "/api/collections/{collection}/request-password-reset",
            post(emails::request_password_reset),
        )
        .route(
            "/api/collections/{collection}/confirm-password-reset",
            post(emails::confirm_password_reset),
        )
        .route(
            "/api/collections/{collection}/request-email-change",
            post(emails::request_email_change),
        )
        .route(
            "/api/collections/{collection}/confirm-email-change",
            post(emails::confirm_email_change),
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
        // The Studio, embedded in the binary. Registered before the layers like
        // every other route, and outside `/api/` so the request logger and the
        // auth middleware stay out of serving a static page.
        .route("/", get(studio::root))
        .route("/_", get(studio::index))
        .route("/_/", get(studio::index))
        .route("/_/{*path}", get(studio::asset))
        .fallback(not_found)
        // Authentication wraps every route, so any handler can read the caller.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::authenticate,
        ))
        // Outside authentication, so a refused request is still logged. Inside
        // CORS, so preflights are answered without a log write.
        .layer(middleware::from_fn_with_state(state.clone(), logs::record))
        // Inside the logger, so a refusal is recorded, and inside CORS, so a
        // preflight is answered without spending an allowance.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::guard,
        ))
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
///
/// What it *does* report is the build that is running (`data.version`, so a probe
/// can answer "which version is actually deployed?"), and a subsystem that has
/// failed locally and cannot fix itself — the schema watcher and the realtime
/// listener. Those are the failures that used to hide: the API kept answering, so
/// the node looked healthy while it served a stale schema and delivered no realtime
/// events. The `unavailable` field is present only when something is wrong.
async fn health() -> Json<Value> {
    let unavailable = lazypock_core::health::unavailable();

    let mut data = serde_json::Map::new();
    // Compile-time, from the workspace version, so a binary built from a release tag
    // reports that tag: a deployment can tell a stale container from a fresh one
    // without shell access. `version` is always present; `unavailable` is not.
    data.insert("version".to_string(), json!(env!("CARGO_PKG_VERSION")));
    if !unavailable.is_empty() {
        data.insert("unavailable".to_string(), json!(unavailable));
    }

    Json(json!({
        "code": 200,
        "message": "OK",
        "data": data,
    }))
}

/// Any unmatched route returns the standard error envelope.
async fn not_found() -> ApiError {
    ApiError::new(Error::not_found("The requested resource was not found."))
}
