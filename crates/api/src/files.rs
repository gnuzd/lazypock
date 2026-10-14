//! File upload, serving and deletion (`PLAN.md` section 10).
//!
//! Uploads and deletions require an authenticated request; serving does not.
//! An object is protected by its **key**, which contains a random component, so
//! it behaves as a capability URL — that is the same model PocketBase uses for
//! its file URLs, and it is why the random prefix is not optional.

use axum::extract::{DefaultBodyLimit, Extension, Multipart, Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use lazypock_core::error::Error;
use lazypock_db::types::validate_identifier;
use lazypock_files::{keys, outbox};
use serde_json::json;

use crate::auth::Principal;
use crate::error::ApiError;
use crate::state::AppState;

/// Largest multipart body accepted.
///
/// Bounds what the server buffers before validation can reject it: the per-field
/// limit is enforced from the field's own options, this is the outer guard.
pub const MAX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

/// The body limit layer for the upload route.
pub fn body_limit() -> DefaultBodyLimit {
    DefaultBodyLimit::max(MAX_UPLOAD_BYTES)
}

/// `POST /api/files` — a multipart upload.
///
/// Parts: `collection`, `record`, and `file`. The object is validated before it
/// is stored, so a rejected upload leaves nothing behind.
pub async fn upload(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    if principal.user_id().is_none() {
        return Err(ApiError::new(Error::forbidden(
            "uploading requires an authenticated request",
        )));
    }

    let mut collection: Option<String> = None;
    let mut record_id: Option<String> = None;
    let mut filename: Option<String> = None;
    let mut declared_type = String::new();
    let mut body: Option<Bytes> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::new(Error::invalid(format!("cannot read the upload: {err}"))))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "collection" => collection = Some(field.text().await.map_err(part_error)?),
            "record" => record_id = Some(field.text().await.map_err(part_error)?),
            "file" => {
                // Read these before the field is consumed by `bytes`.
                filename = field.file_name().map(str::to_string);
                declared_type = field.content_type().unwrap_or_default().to_string();
                body = Some(field.bytes().await.map_err(part_error)?);
            }
            // Unknown parts are ignored rather than fatal: a client may send
            // more than we need.
            _ => {}
        }
    }

    let collection = collection
        .ok_or_else(|| ApiError::new(Error::invalid("a `collection` part is required")))?;
    let record_id =
        record_id.ok_or_else(|| ApiError::new(Error::invalid("a `record` part is required")))?;
    let filename = filename.unwrap_or_else(|| "upload".to_string());
    let body = body.ok_or_else(|| ApiError::new(Error::invalid("a `file` part is required")))?;

    // The collection has to exist, so an upload cannot create an object that
    // nothing will ever reference and no orphan sweep could attribute.
    if state.schema.snapshot().get(&collection).is_none() {
        return Err(ApiError::new(Error::not_found(format!(
            "no collection named {collection:?}"
        ))));
    }

    let key = keys::storage_key(&collection, &record_id, &filename).map_err(ApiError::new)?;
    let format = state
        .files
        .store(&key, &declared_type, body)
        .await
        .map_err(ApiError::new)?;

    let name = key.rsplit('/').next().unwrap_or_default().to_string();
    tracing::info!(collection = %collection, key = %key, "file stored");

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "key": key,
            "collection": collection,
            "record": record_id,
            "type": format.as_str(),
            "url": state.files.url(&collection, &record_id, &name),
        })),
    )
        .into_response())
}

/// `GET /api/files/{collection}/{record}/{filename}`
pub async fn serve(
    State(state): State<AppState>,
    Path((collection, record_id, filename)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    // The path segments become a key, so they are checked exactly as an upload's
    // would be: a traversal here would read outside the storage root.
    validate_identifier("collection name", &collection).map_err(ApiError::new)?;
    let key = format!(
        "{collection}/{}/{}",
        safe_segment(&record_id)?,
        safe_segment(&filename)?
    );

    let object = state
        .files
        .storage()
        .get(&key)
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| ApiError::new(Error::not_found("no such file")))?;

    let mut response = (StatusCode::OK, object.body).into_response();
    let content_type = HeaderValue::from_str(&object.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    // The key contains a random component and an object at a key never changes,
    // so the response can be cached indefinitely.
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(response)
}

/// `DELETE /api/files/{collection}/{record}/{filename}`
///
/// The object is removed through the outbox, so a crash between the queue entry
/// and the deletion cannot lose it.
pub async fn delete(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((collection, record_id, filename)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    if principal.user_id().is_none() {
        return Err(ApiError::new(Error::forbidden(
            "deleting a file requires an authenticated request",
        )));
    }

    let key = format!(
        "{collection}/{}/{}",
        safe_segment(&record_id)?,
        safe_segment(&filename)?
    );

    let client = state.db.client().await.map_err(ApiError::new)?;
    outbox::enqueue(&client, &key)
        .await
        .map_err(ApiError::new)?;
    let report = outbox::drain(&client, state.files.storage().as_ref())
        .await
        .map_err(ApiError::new)?;

    if report.deleted == 0 && report.failed == 0 {
        // Nothing was queued for deletion, so the object was not there.
        return Err(ApiError::new(Error::not_found("no such file")));
    }

    tracing::info!(key = %key, "file deleted");
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// A path segment that is safe inside a storage key.
fn safe_segment(segment: &str) -> Result<String, ApiError> {
    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.contains(['/', '\\', '\0'])
    {
        return Err(ApiError::new(Error::invalid(
            "the file path contains an unsafe segment",
        )));
    }
    Ok(segment.to_string())
}

fn part_error(err: axum::extract::multipart::MultipartError) -> ApiError {
    ApiError::new(Error::invalid(format!("cannot read the upload: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_path_segments_are_refused() {
        assert!(safe_segment("photo.png").is_ok());
        for bad in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
            assert!(safe_segment(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn the_upload_limit_is_a_sane_multiple_of_the_field_default() {
        // Larger than the default per-field limit, and small enough that a
        // rejected upload cannot exhaust memory first.
        const { assert!(MAX_UPLOAD_BYTES >= 5 * 1024 * 1024) };
        const { assert!(MAX_UPLOAD_BYTES <= 64 * 1024 * 1024) };
    }
}
