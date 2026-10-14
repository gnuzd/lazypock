//! Backup and restore over HTTP.
//!
//! Superuser-only throughout: an export is the whole database, and an import
//! writes to it.
//!
//! The import gate is a **409 with a body**, not an `ApiError`, because it is not a
//! failure — it is the server asking whether a large import was meant. Its shape is
//! the one the Studio import page already handles: `data.requires_confirmation`
//! plus the preflight, so a client can show what would happen and ask.

use axum::extract::{DefaultBodyLimit, Extension, Multipart, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use lazypock_core::error::Error;
use lazypock_core::id::new_id;
use lazypock_db::backup::{self, ImportOptions, ImportOutcome};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{Principal, Superuser};
use crate::error::ApiError;
use crate::state::AppState;

/// The largest archive accepted.
///
/// Generous, because a backup of a real database is large, but bounded: the body is
/// buffered before it can be parsed.
pub const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;

/// The body limit for the import route.
pub fn body_limit() -> DefaultBodyLimit {
    DefaultBodyLimit::max(MAX_ARCHIVE_BYTES)
}

/// Records above which an import must be confirmed, from
/// `LAZYPOCK_IMPORT_CONFIRM_RECORDS`.
pub fn confirm_threshold() -> Result<u64, ApiError> {
    match std::env::var("LAZYPOCK_IMPORT_CONFIRM_RECORDS") {
        Err(_) => Ok(10_000),
        Ok(raw) => raw.trim().parse().map_err(|_| {
            ApiError::new(Error::config(format!(
                "LAZYPOCK_IMPORT_CONFIRM_RECORDS must be a whole number, got {raw:?}"
            )))
        }),
    }
}

/// `GET /api/export/archive`
pub async fn export(State(state): State<AppState>, _: Superuser) -> Result<Response, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let archive = backup::export(&client).await.map_err(ApiError::new)?;
    let bytes = backup::write(&archive).map_err(ApiError::new)?;

    let name = format!(
        "lazypock-backup-{}.zip",
        archive
            .manifest
            .created_at
            .as_deref()
            .unwrap_or("archive")
            .replace(':', "-")
    );

    tracing::info!(
        collections = archive.schema.collections.len(),
        records = archive.record_count(),
        bytes = bytes.len(),
        "exported an archive"
    );

    let mut response = (StatusCode::OK, bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    Ok(response)
}

/// `GET /api/export`
///
/// The single-document JSON backup — the same collections and records as
/// `/api/export/archive`, in one object. Studio's "Download JSON" button reads this
/// shape, and the pre-import snapshot stores it.
pub async fn export_json(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let archive = backup::export(&client).await.map_err(ApiError::new)?;

    tracing::info!(
        collections = archive.schema.collections.len(),
        records = archive.record_count(),
        "exported a JSON backup"
    );

    Ok(Json(backup::to_payload(&archive)))
}

/// `GET /api/import/preflight`
///
/// What an import would be gated on, before any file is sent. The per-archive
/// preflight comes back from the import call itself, since only that has the file.
pub async fn preflight(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let collections = lazypock_db::collections::list(&client)
        .await
        .map_err(ApiError::new)?;

    Ok(Json(json!({
        "available": true,
        "confirm_records": confirm_threshold()?,
        "collections": collections.len(),
    })))
}

/// `POST /api/import`
///
/// Multipart: `file`, `atomic`, and `confirm`. Returns the report, or the gate.
pub async fn import(
    State(state): State<AppState>,
    _: Superuser,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let mut file: Option<bytes::Bytes> = None;
    let mut atomic = String::new();
    let mut confirm = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::new(Error::invalid(format!("cannot read the upload: {err}"))))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "file" => {
                file = Some(field.bytes().await.map_err(|err| {
                    ApiError::new(Error::invalid(format!("cannot read the archive: {err}")))
                })?);
            }
            "atomic" => {
                atomic = field.text().await.map_err(|err| {
                    ApiError::new(Error::invalid(format!("cannot read `atomic`: {err}")))
                })?;
            }
            "confirm" => {
                let value = field.text().await.map_err(|err| {
                    ApiError::new(Error::invalid(format!("cannot read `confirm`: {err}")))
                })?;
                confirm = matches!(value.trim(), "true" | "1" | "yes" | "on");
            }
            // An unknown part is refused rather than ignored: a client sending
            // `deleteMissing` should be told the server has no such option, not have
            // it silently dropped while believing its intent was honoured.
            other => {
                return Err(ApiError::new(Error::invalid(format!(
                    "unknown import option {other:?}; the server deletes nothing on import, \
                     so there is no option for it"
                ))))
            }
        }
    }

    let file = file.ok_or_else(|| ApiError::new(Error::invalid("a `file` part is required")))?;
    let atomic = if atomic.trim().is_empty() {
        backup::AtomicMode::Batch
    } else {
        backup::AtomicMode::parse(atomic.trim()).map_err(ApiError::new)?
    };

    let archive = backup::read(&file).map_err(ApiError::new)?;
    let options = ImportOptions {
        atomic,
        confirm,
        confirm_threshold: confirm_threshold()?,
    };

    let mut client = state.db.client().await.map_err(ApiError::new)?;

    // The undo checkpoint, taken before anything is written. A snapshot that cannot
    // be captured fails the import rather than letting it proceed with no way back.
    capture_snapshot(&client).await?;

    match backup::import(&mut client, &archive, &options)
        .await
        .map_err(ApiError::new)?
    {
        ImportOutcome::Applied(report) => Ok((StatusCode::OK, Json(json!(report))).into_response()),
        ImportOutcome::NeedsConfirmation(preflight) => {
            // Not an error: the server is asking a question. Shaped the way the
            // import page already reads it.
            Ok((
                StatusCode::CONFLICT,
                Json(json!({
                    "status": 409,
                    "message": format!(
                        "this import holds {} records; confirm it to proceed",
                        preflight.total_records
                    ),
                    "data": {
                        "requires_confirmation": true,
                        "preflight": *preflight,
                    },
                })),
            )
                .into_response())
        }
    }
}

/// `GET /api/import/status`
///
/// Whether an undo checkpoint exists — what the Studio's "Undo last import"
/// renders from. There is at most one, so nothing is paginated.
pub async fn import_status(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let row = client
        .query_opt(
            "select to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as created_at \
               from _import_snapshots order by created_at desc, id desc limit 1",
            &[],
        )
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;

    let snapshot = row.map(|row| json!({ "created_at": row.get::<_, String>("created_at") }));
    Ok(Json(json!({ "snapshot": snapshot })))
}

/// The body `POST /api/import/rollback` accepts.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollbackBody {
    /// The acting superuser's password.
    #[serde(default)]
    pub password: Option<String>,
}

/// `POST /api/import/rollback`
///
/// Destructive by design: undoing an import means removing what it added, which is
/// the one thing `/api/import` will not do. Password confirmation is required on top
/// of the superuser token, because this is a whole-database operation.
pub async fn import_rollback(
    State(state): State<AppState>,
    _: Superuser,
    Extension(principal): Extension<Principal>,
    Json(body): Json<RollbackBody>,
) -> Result<Json<Value>, ApiError> {
    let password = body.password.unwrap_or_default();
    if password.is_empty() {
        return Err(ApiError::new(Error::unauthorized(
            "password confirmation is required",
        )));
    }

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    let id = principal
        .user_id()
        .ok_or_else(|| ApiError::new(Error::unauthorized("no superuser identity")))?;
    let account = lazypock_auth::superuser::find_by_id(&client, id)
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| ApiError::new(Error::unauthorized("the superuser no longer exists")))?;

    if state
        .auth
        .authenticate_superuser(&account.email, &password)
        .await
        .is_err()
    {
        return Err(ApiError::new(Error::unauthorized("invalid password")));
    }

    let row = client
        .query_opt(
            "select payload from _import_snapshots order by created_at desc, id desc limit 1",
            &[],
        )
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;
    let Some(row) = row else {
        return Err(ApiError::new(Error::not_found(
            "there is no import snapshot to roll back to",
        )));
    };
    let payload: Value = row.get("payload");

    let report = backup::restore_snapshot(&mut client, &payload)
        .await
        .map_err(ApiError::new)?;

    // The checkpoint is spent: a second rollback would restore the same state, and
    // the Studio should stop offering the action.
    client
        .execute("delete from _import_snapshots", &[])
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;

    tracing::warn!(
        collections = report.imported.len(),
        "rolled back the last import"
    );
    Ok(Json(json!({
        "rolled_back": true,
        "imported": report.imported,
        "errors": report.errors,
    })))
}

/// Capture the undo checkpoint, replacing any previous one.
async fn capture_snapshot(client: &lazypock_db::deadpool_postgres::Client) -> Result<(), ApiError> {
    let archive = backup::export(client).await.map_err(ApiError::new)?;
    let payload = backup::to_payload(&archive);

    // Only the latest is kept: the client's action is "undo the last import", and a
    // full copy of the database per import is not something to accumulate.
    client
        .execute("delete from _import_snapshots", &[])
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;
    client
        .execute(
            "insert into _import_snapshots (id, payload) values ($1, $2)",
            &[&new_id(), &payload],
        )
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;
    Ok(())
}
