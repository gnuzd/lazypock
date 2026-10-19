//! The admin action audit log over `_audit`.
//!
//! The table has existed since migration 1 with an index on `created`, and nothing
//! wrote to it — so an admin action left no trace anywhere, which is the same
//! shape as `_jobs` before its worker. This is the writer and the reader.
//!
//! What is recorded is the set of actions that change *who may do what*: settings
//! (including storage credentials), API keys, collection definitions, and the two
//! destructive import paths. Reads are deliberately not audited — the request log
//! already records every `/api/*` request, and copying that here would bury the
//! changes in it.
//!
//! A change that cannot be recorded is not made: `record` is called on the same
//! runner as the change, so where the caller has a transaction the two commit
//! together, and a failure to record fails the action. An unattributable change is
//! worse than a refused one.

use axum::extract::{Query, State};
use axum::Json;
use lazypock_core::error::Error;
use lazypock_db::pool::db_error;
use lazypock_db::runner::QueryRunner;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{Principal, Superuser};
use crate::error::ApiError;
use crate::state::AppState;

/// Rows a page holds when the caller does not say.
const DEFAULT_PER_PAGE: i64 = 50;

/// The most a page may hold, so one request cannot ask for the whole log.
const MAX_PER_PAGE: i64 = 200;

/// The actions worth recording.
///
/// String constants rather than free text at each call site, so a typo shows up as
/// a compile error instead of as an action nobody can query for.
pub mod action {
    /// A collection definition was created.
    pub const COLLECTION_CREATE: &str = "collection.create";
    /// A collection definition changed.
    pub const COLLECTION_UPDATE: &str = "collection.update";
    /// A collection definition was deleted.
    pub const COLLECTION_DELETE: &str = "collection.delete";
    /// Settings changed.
    pub const SETTINGS_UPDATE: &str = "settings.update";
    /// An API key was issued.
    pub const API_KEY_CREATE: &str = "api_key.create";
    /// An API key was revoked.
    pub const API_KEY_REVOKE: &str = "api_key.revoke";
    /// An import replaced the database's contents.
    pub const IMPORT: &str = "import";
    /// An import was undone.
    pub const IMPORT_ROLLBACK: &str = "import.rollback";
    /// A cron job was created, changed or removed.
    pub const CRON_CHANGE: &str = "cron.change";
    /// A file preset was created, changed or removed.
    pub const PRESET_CHANGE: &str = "preset.change";
    /// A cron job was run by hand.
    pub const CRON_RUN: &str = "cron.run";

    /// A message queued through `POST /api/mail`.
    pub const MAIL_SEND: &str = "mail.send";
}

/// The acting superuser's id, for [`record`].
///
/// A superuser always has one. The fallback is empty rather than a panic: an entry
/// with no actor is still a record that something happened, and losing the trail is
/// worse than an incomplete line in it.
pub fn actor(principal: &Principal) -> String {
    principal.user_id().unwrap_or_default().to_string()
}

/// Record an admin action.
///
/// `actor` is the superuser's record id, `target` names what was acted on (a
/// collection name, an API key id, …), and `details` carries whatever makes the
/// entry useful without repeating the target.
pub async fn record(
    runner: &dyn QueryRunner,
    actor: &str,
    action: &str,
    target: Option<&str>,
    details: Value,
) -> Result<(), Error> {
    runner
        .run_execute(
            "insert into _audit (actor, action, target, details) values ($1, $2, $3, $4)",
            &[
                &actor.to_string(),
                &action.to_string(),
                &target.map(str::to_string),
                &details,
            ],
        )
        .await
        .map_err(db_error)?;

    tracing::info!(actor = %actor, action = %action, target = ?target, "admin action");
    Ok(())
}

/// What `GET /api/audit` accepts.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    /// 1-based page number.
    #[serde(default)]
    pub page: Option<i64>,
    /// Rows per page, capped at [`MAX_PER_PAGE`].
    #[serde(default)]
    pub per_page: Option<i64>,
}

/// `GET /api/audit` — newest first, superuser-only.
///
/// Superuser-only like the request log, and for the same reason: it names who did
/// what and when.
pub async fn list(
    State(state): State<AppState>,
    _: Superuser,
    Query(params): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let per_page = params
        .per_page
        .unwrap_or(DEFAULT_PER_PAGE)
        .clamp(1, MAX_PER_PAGE);
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * per_page;

    let client = state.db.client().await.map_err(ApiError::new)?;

    // Ordered by `id` rather than `created`: two entries in the same millisecond
    // would otherwise come back in an arbitrary order, and the log is only useful
    // if the sequence is real. `id` is a bigserial, so it is that sequence.
    let rows = client
        .query(
            "select id, actor, action, target, details, \
                    to_char(created at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') \
                      as created \
               from _audit order by id desc limit $1 offset $2",
            &[&per_page, &offset],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let total: i64 = client
        .query_one("select count(*) as n from _audit", &[])
        .await
        .map_err(|err| ApiError::new(db_error(err)))?
        .get("n");

    let items: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "id": row.get::<_, i64>("id"),
                "actor": row.get::<_, Option<String>>("actor"),
                "action": row.get::<_, String>("action"),
                "target": row.get::<_, Option<String>>("target"),
                "details": row.get::<_, Value>("details"),
                "created": row.get::<_, Option<String>>("created"),
            })
        })
        .collect();

    Ok(Json(json!({
        "page": page,
        "perPage": per_page,
        "totalItems": total,
        "totalPages": (total + per_page - 1) / per_page,
        "items": items,
    })))
}
