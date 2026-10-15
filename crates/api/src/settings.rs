//! Application settings.
//!
//! Stored as key/value rows in `_settings`, and read and written as one flat
//! object. The keys are the client's vocabulary — `app_name`, `cors_origins`,
//! `mail`, `storage` — so the server stores them generically rather than
//! enumerating them. A settings page added on the client therefore works without a
//! server change, which is the property that matters here: the alternative is a
//! server release every time a form gains a field.
//!
//! Superuser-only. Settings include secrets — SMTP credentials, storage keys — so
//! this is configuration, not content.

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use axum::response::Json as ResponseJson;
use axum::response::{IntoResponse, Response};
use lazypock_core::error::Error;
use lazypock_files::FilesConfig;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::api_keys;
use crate::auth::Superuser;
use crate::error::ApiError;
use crate::state::AppState;

/// `GET /api/settings`
///
/// Every stored setting, flat. A setting that was never written is simply absent
/// rather than null, so a client can tell "unset" from "set to null".
pub async fn get(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<ResponseJson<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let rows = client
        .query("select key, value from _settings order by key", &[])
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;

    let mut settings = Map::new();
    for row in &rows {
        let key: String = row.get("key");
        // Lazypock's own settings are not application settings, and one of them is
        // the signing key.
        if is_internal(&key) {
            continue;
        }
        let value: Value = row.get("value");
        settings.insert(key, value);
    }

    Ok(ResponseJson(Value::Object(settings)))
}

/// `PATCH /api/settings`
///
/// Upserts the keys given and leaves every other key alone, so a form can save one
/// group without sending the rest back.
pub async fn update(
    State(state): State<AppState>,
    _: Superuser,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Result<ResponseJson<Value>, ApiError> {
    let Json(body) = crate::auth::json_body(body)?;
    let Some(object) = body.as_object() else {
        return Err(ApiError::new(Error::invalid(
            "settings must be a JSON object",
        )));
    };

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    let transaction = client
        .transaction()
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;

    for (key, value) in object {
        if is_internal(key) {
            return Err(ApiError::new(Error::invalid(format!(
                "{key:?} is lazypock's own setting and cannot be written through the \
                 settings API"
            ))));
        }

        // A null means "clear this setting", which is a delete rather than a row
        // holding null: an absent key and a cleared one then read the same, which
        // is what a client expects from a settings form.
        if value.is_null() {
            transaction
                .execute("delete from _settings where key = $1", &[key])
                .await
                .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;
            continue;
        }

        transaction
            .execute(
                "insert into _settings (key, value, updated) values ($1, $2, now()) \
                 on conflict (key) do update set value = excluded.value, updated = now()",
                &[key, value],
            )
            .await
            .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;
    }

    transaction
        .commit()
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;

    tracing::info!(keys = object.len(), "settings updated");
    Ok(ResponseJson(
        json!({ "updated": object.keys().cloned().collect::<Vec<_>>() }),
    ))
}

/// The prefix reserved for lazypock's own settings.
///
/// `_settings` holds more than application settings: the JWT signing key lives
/// there too. A settings endpoint that returned everything would hand out the
/// credential that mints every token — which is exactly what the first version of
/// this endpoint did, caught by a test asserting the object was empty on a fresh
/// installation. Writes are refused as well, because overwriting the signing key
/// through a settings form would break authentication just as completely.
const INTERNAL_PREFIX: &str = "auth.";

/// Whether a key belongs to lazypock rather than to the application.
fn is_internal(key: &str) -> bool {
    key.starts_with(INTERNAL_PREFIX)
}

/// Why a storage write is refused, naming every variable that has to change.
const STORAGE_IS_ENVIRONMENT_ONLY: &str = "storage is configured by the environment, not in the \
     database: set LAZYPOCK_FILES_ADAPTER, LAZYPOCK_FILES_DIR, LAZYPOCK_S3_ENDPOINT, \
     LAZYPOCK_S3_BUCKET, LAZYPOCK_S3_REGION, LAZYPOCK_S3_ACCESS_KEY, LAZYPOCK_S3_SECRET_KEY \
     and LAZYPOCK_FILES_PUBLIC_URL, then restart. Keeping a storage credential out of the \
     database is deliberate: a backup, a replica or the SQL console would otherwise carry it.";

/// `GET /api/settings/storage`
///
/// The storage configuration the server is **running with**. The Studio's Files Storage
/// page reads this, and a 404 there reads as a broken server rather than as a design.
///
/// The secret is reported as `secret_set` and never echoed, and `configured_from_env`
/// names every field, which is what tells the page to show the active configuration
/// rather than a form whose save would do nothing.
pub async fn storage_get(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<ResponseJson<Value>, ApiError> {
    Ok(ResponseJson(storage_view(
        state.files.config(),
        state.files.backend_name(),
    )))
}

/// `PATCH /api/settings/storage`
///
/// Refused, loudly, with the variables that have to change instead. Accepting a write
/// that changed nothing would be the one outcome worse than refusing it.
pub async fn storage_update(_: Superuser) -> Result<ResponseJson<Value>, ApiError> {
    Err(ApiError::new(Error::conflict(STORAGE_IS_ENVIRONMENT_ONLY)))
}

/// `POST /api/settings/storage/test`
///
/// A real round trip against the configured backend — the local adapter creates and
/// probes its directory, S3 lists the bucket — reported as steps, so the page can say
/// which part failed instead of "the connection failed".
pub async fn storage_test(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<Response, ApiError> {
    let backend = state.files.backend_name();

    match state.files.storage().health().await {
        Ok(()) => Ok((
            StatusCode::OK,
            ResponseJson(json!({
                "ok": true,
                "steps": [{ "step": backend, "ok": true, "error": Value::Null }],
            })),
        )
            .into_response()),
        Err(err) => {
            let message = format!("the {backend} backend is not usable: {}", err.message());
            tracing::warn!(backend, error = %err, "storage test failed");
            let steps = json!([{ "step": backend, "ok": false, "error": err.message() }]);
            // The steps are carried at both levels: the page's error path reads them from
            // the body it holds, and the body's own shape is the API's `{message, data}`
            // envelope.
            Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                ResponseJson(json!({
                    "status": 422,
                    "message": message,
                    "steps": steps,
                    "data": { "steps": steps, "message": message },
                })),
            )
                .into_response())
        }
    }
}

/// The storage configuration as the Studio's page reads it.
fn storage_view(config: &FilesConfig, backend: &str) -> Value {
    json!({
        "backend": backend,
        "endpoint": config.endpoint.clone().unwrap_or_default(),
        "region": config.region.clone(),
        "bucket": config.bucket.clone().unwrap_or_default(),
        "access_key_id": config.access_key.clone().unwrap_or_default(),
        // Never the secret, only whether there is one: the page has a field for it and a
        // blank one means "unchanged".
        "secret_access_key": "",
        "secret_set": config.secret_key.is_some(),
        // There is no prefix: an object's key is `{collection}/{record}/{filename}`, and
        // saying so is better than accepting one and ignoring it.
        "prefix": "",
        "public_base_url": config.public_base_url.clone().unwrap_or_default(),
        // Path style always: a URL is built as `<endpoint>/<bucket>/<key>`.
        "force_path_style": true,
        "presign_ttl": crate::files::PRESIGN_TTL_SECONDS,
        // Every field, because every one comes from the environment. The page locks what
        // this names, which is what turns a form into a report.
        "configured_from_env": [
            "backend",
            "endpoint",
            "region",
            "bucket",
            "access_key_id",
            "secret_access_key",
            "prefix",
            "public_base_url",
            "force_path_style",
            "presign_ttl",
        ],
    })
}

/// `POST /api/settings/refresh-cors`
///
/// Reload the CORS allowlist from settings. Studio calls this straight after saving
/// `cors_origins` so the change takes effect immediately, and it exists because the
/// enforcement does — an endpoint that accepted this call and changed nothing would
/// report success while leaving the old origins in force.
pub async fn refresh_cors(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<ResponseJson<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    state.cors.load(&client).await.map_err(ApiError::new)?;
    let origins = state.cors.origins();
    tracing::info!(?origins, "CORS allowlist refreshed");
    Ok(ResponseJson(json!({ "cors_origins": origins })))
}

/// The body `POST /api/settings/api-keys` accepts.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateApiKey {
    /// Days until the key expires; absent or `0` means it does not.
    #[serde(default)]
    pub expires_in_days: Option<i64>,
}

/// `GET /api/settings/api-keys`
///
/// The stored keys, without their digests — a client only needs an id to revoke and
/// the dates to display.
pub async fn list_api_keys(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<ResponseJson<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    Ok(ResponseJson(
        json!({ "items": public_items(&client).await? }),
    ))
}

/// `POST /api/settings/api-keys`
///
/// The raw key is in the response **once**; only its digest is stored, so it can
/// never be shown again.
pub async fn generate_api_key(
    State(state): State<AppState>,
    _: Superuser,
    Json(body): Json<CreateApiKey>,
) -> Result<ResponseJson<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let (raw, key) = api_keys::create(&client, body.expires_in_days).await?;

    tracing::info!(id = %key.id, "API key created");
    Ok(ResponseJson(json!({
        "api_key": raw,
        "items": public_items(&client).await?,
    })))
}

/// `DELETE /api/settings/api-keys/{id}`
///
/// Revokes rather than deletes, so the list still shows that the key existed and
/// was revoked.
pub async fn revoke_api_key(
    State(state): State<AppState>,
    _: Superuser,
    Path(id): Path<String>,
) -> Result<ResponseJson<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    if !api_keys::revoke(&client, &id).await? {
        return Err(ApiError::new(Error::not_found(format!(
            "no API key with id {id:?}"
        ))));
    }
    tracing::info!(id = %id, "API key revoked");
    Ok(ResponseJson(json!({ "ok": true })))
}

/// Every key in the shape a client reads.
async fn public_items(
    client: &lazypock_db::deadpool_postgres::Client,
) -> Result<Vec<Value>, ApiError> {
    Ok(api_keys::list(client)
        .await?
        .iter()
        .map(api_keys::ApiKey::public)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazypocks_own_settings_are_never_exposed_or_written() {
        // The signing key lives in `_settings`. It must not come out of a settings
        // read, and must not go in through a settings write.
        assert!(is_internal("auth.signing_key"));
        assert!(is_internal("auth.anything"));

        // Application settings, including ones that look auth-ish, are ordinary.
        assert!(!is_internal("app_name"));
        assert!(!is_internal("cors_origins"));
        assert!(!is_internal("mail"));
        assert!(!is_internal("authentication"));
    }
}
