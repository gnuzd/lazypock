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
use axum::response::Json as ResponseJson;
use lazypock_core::error::Error;
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
