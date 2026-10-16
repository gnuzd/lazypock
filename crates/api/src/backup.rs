//! Backup and restore over HTTP.
//!
//! Superuser-only throughout: an export is the whole database, and an import
//! writes to it.
//!
//! The import gate is a **409 with a body**, not an `ApiError`, because it is not a
//! failure — it is the server asking whether a large import was meant. Its shape is
//! the one the Studio import page already handles: `data.requires_confirmation`
//! plus the preflight, so a client can show what would happen and ask.

use axum::extract::{DefaultBodyLimit, Extension, FromRequest, Multipart, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use lazypock_core::error::Error;
use lazypock_core::id::new_id;
use lazypock_db::backup::{self, ImportOptions, ImportOutcome};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::audit;
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

/// The JSON body `POST /api/import` accepts, which is what a client restores a
/// downloaded JSON backup with.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonImport {
    /// The `{collections: [...]}` envelope, or a bare array.
    #[serde(default)]
    pub collections: Option<Value>,
    /// `batch`, `per_collection`, `continue`, or `false`.
    #[serde(default)]
    pub atomic: Option<Value>,
    /// Whether a gated import is confirmed.
    #[serde(default)]
    pub confirm: Option<bool>,
    /// Whether the import drops collections and fields the payload leaves out.
    ///
    /// The Studio's "Delete missing collections and schema fields" switch. Off when
    /// absent, which is what makes importing a partial file safe.
    #[serde(default)]
    pub delete_missing: Option<bool>,
    /// Accepted and ignored: this confirms a rollback, not an import.
    #[serde(default)]
    pub password: Option<String>,
}

/// `POST /api/import`
///
/// Two shapes: a multipart archive upload, or the single-document JSON a client
/// downloaded from `GET /api/export`. Both accept `deleteMissing` and `password`;
/// `deleteMissing` is honoured (it drops user collections and fields the payload
/// leaves out) and the password is not consumed here, because it confirms a
/// rollback rather than an import. An option that *is* ignored is reported back
/// rather than silently dropped.
pub async fn import(
    State(state): State<AppState>,
    superuser: Superuser,
    request: Request,
) -> Result<Response, ApiError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();

    let (archive, atomic, confirm, delete_missing, ignored) = if content_type
        .starts_with("application/json")
    {
        let bytes = axum::body::to_bytes(request.into_body(), MAX_ARCHIVE_BYTES)
            .await
            .map_err(|err| ApiError::new(Error::invalid(format!("cannot read the body: {err}"))))?;
        let body: JsonImport = serde_json::from_slice(&bytes).map_err(|err| {
            ApiError::new(Error::invalid(format!("invalid import payload: {err}")))
        })?;

        let collections = body
            .collections
            .ok_or_else(|| ApiError::new(Error::invalid("an import needs `collections`")))?;
        let archive = backup::archive_from_payload(&collections).map_err(ApiError::new)?;
        let atomic = match body.atomic {
            None | Some(Value::Bool(false)) => String::new(),
            Some(Value::String(value)) => value,
            Some(Value::Bool(true)) => "batch".to_string(),
            Some(other) => {
                return Err(ApiError::new(Error::invalid(format!(
                    "`atomic` must be a string or false, got {other}"
                ))))
            }
        };
        (
            archive,
            atomic,
            body.confirm.unwrap_or(false),
            body.delete_missing.unwrap_or(false),
            // The password confirms a rollback, not an import, so a JSON restore
            // that carries one is told it went unused rather than left to think it
            // did something.
            body.password.is_some().then(|| "password".to_string()),
        )
    } else {
        let multipart = Multipart::from_request(request, &state)
            .await
            .map_err(|err| {
                ApiError::new(Error::invalid(format!("cannot read the upload: {err}")))
            })?;
        parse_multipart(multipart).await?
    };

    let atomic = if atomic.trim().is_empty() {
        backup::AtomicMode::Batch
    } else {
        backup::AtomicMode::parse(atomic.trim()).map_err(ApiError::new)?
    };

    let options = ImportOptions {
        atomic,
        confirm,
        confirm_threshold: confirm_threshold()?,
        delete_missing,
    };

    let mut client = state.db.client().await.map_err(ApiError::new)?;

    // The undo checkpoint, taken before anything is written. A snapshot that cannot
    // be captured fails the import rather than letting it proceed with no way back.
    capture_snapshot(&client).await?;

    match backup::import(&mut client, &archive, &options)
        .await
        .map_err(ApiError::new)?
    {
        ImportOutcome::Applied(report) => {
            // Recorded only when something was applied: `NeedsConfirmation` above
            // did not change anything, so there is nothing to have done. An import
            // can drop collections, so their names are kept rather than only
            // counted.
            audit::record(
                &client,
                &superuser.id,
                audit::action::IMPORT,
                None,
                json!({
                    "collections": report.imported.len(),
                    "skipped": report.skipped.len(),
                    "errors": report.errors.len(),
                    "removed": report.removed,
                    "atomic": options.atomic.as_str(),
                }),
            )
            .await
            .map_err(ApiError::new)?;

            let mut body = serde_json::to_value(&report).map_err(|err| {
                ApiError::new(Error::internal(format!(
                    "cannot serialize the import report: {err}"
                )))
            })?;
            if let Some(option) = &ignored {
                body["ignoredOptions"] = json!([option]);
            }
            Ok((StatusCode::OK, Json(body)).into_response())
        }
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

/// Read the multipart archive upload, returning the archive and its options.
async fn parse_multipart(
    mut multipart: Multipart,
) -> Result<(backup::Archive, String, bool, bool, Option<String>), ApiError> {
    let mut file: Option<bytes::Bytes> = None;
    let mut atomic = String::new();
    let mut confirm = false;
    let mut delete_missing = false;
    let mut ignored: Option<String> = None;

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
                confirm = is_true(&value);
            }
            "deleteMissing" => {
                let value = field.text().await.map_err(|err| {
                    ApiError::new(Error::invalid(format!(
                        "cannot read `deleteMissing`: {err}"
                    )))
                })?;
                delete_missing = is_true(&value);
            }
            // Accepted and ignored rather than rejected: Studio sends it, and it
            // confirms a rollback, not an import. What was ignored is reported back,
            // so it is not silent.
            "password" => {
                let _ = field.text().await;
                ignored = Some("password".to_string());
            }
            other => {
                return Err(ApiError::new(Error::invalid(format!(
                    "unknown import option {other:?}"
                ))))
            }
        }
    }

    let file = file.ok_or_else(|| ApiError::new(Error::invalid("a `file` part is required")))?;
    let archive = backup::read(&file).map_err(ApiError::new)?;
    Ok((archive, atomic, confirm, delete_missing, ignored))
}

/// Whether a form field or query value spells "true".
fn is_true(value: &str) -> bool {
    matches!(value.trim(), "true" | "1" | "yes" | "on")
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

    audit::record(
        &client,
        id,
        audit::action::IMPORT_ROLLBACK,
        None,
        json!({
            "collections": report.imported.len(),
            "errors": report.errors.len(),
        }),
    )
    .await
    .map_err(ApiError::new)?;
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
