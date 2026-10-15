//! File upload, serving and deletion (`PLAN.md` section 10).
//!
//! Uploads and deletions require an authenticated request; serving does not.
//! An object is protected by its **key**, which contains a random component, so
//! it behaves as a capability URL — that is the same model PocketBase uses for
//! its file URLs, and it is why the random prefix is not optional.

use axum::extract::{DefaultBodyLimit, Extension, Multipart, Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use lazypock_core::error::Error;
use lazypock_db::pool::db_error;
use lazypock_db::runner::QueryRunner;
use lazypock_db::types::validate_identifier;
use lazypock_files::{keys, outbox, presets, thumbnail, ImagePreset};
use serde::Deserialize;
use serde_json::{json, Value};

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

    // The record has to exist. The sweeper reclaims any object whose record is
    // gone, so without this an upload for a record that does not exist yet would
    // be reclaimed before it was ever referenced.
    if !record_exists(&state, &collection, &record_id).await? {
        return Err(ApiError::new(Error::not_found(format!(
            "no record {record_id:?} in the collection {collection:?}"
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

/// Query parameters accepted when serving a file.
#[derive(Debug, Default, Deserialize)]
pub struct ServeQuery {
    /// An ad-hoc size, e.g. `?thumb=400x300`.
    pub thumb: Option<String>,
    /// A stored preset, e.g. `?preset=sm`.
    pub preset: Option<String>,
}

/// `GET /api/files/{collection}/{record}/{filename}`
pub async fn serve(
    State(state): State<AppState>,
    Path((collection, record_id, filename)): Path<(String, String, String)>,
    Query(query): Query<ServeQuery>,
) -> Result<Response, ApiError> {
    serve_path(
        state,
        collection,
        record_id,
        filename,
        query.thumb,
        query.preset,
    )
    .await
}

/// `GET /api/files/{collection}/{record}/{filename}/thumbs/{preset}`
pub async fn serve_thumbnail(
    State(state): State<AppState>,
    Path((collection, record_id, filename, preset)): Path<(String, String, String, String)>,
) -> Result<Response, ApiError> {
    serve_path(state, collection, record_id, filename, None, Some(preset)).await
}

/// Serve an object, optionally resized.
async fn serve_path(
    state: AppState,
    collection: String,
    record_id: String,
    filename: String,
    thumb: Option<String>,
    preset: Option<String>,
) -> Result<Response, ApiError> {
    let preset = match (thumb, preset) {
        (Some(_), Some(_)) => {
            return Err(ApiError::new(Error::invalid(
                "give either `thumb` or `preset`, not both",
            )))
        }
        (Some(specification), None) => {
            Some(ImagePreset::parse_size(&specification).map_err(ApiError::new)?)
        }
        (None, Some(name)) => {
            let client = state.db.client().await.map_err(ApiError::new)?;
            Some(
                presets::find(&client, &name)
                    .await
                    .map_err(ApiError::new)?
                    .ok_or_else(|| {
                        ApiError::new(Error::not_found(format!("no image preset named {name:?}")))
                    })?,
            )
        }
        (None, None) => None,
    };

    // The path segments become a key, so they are checked exactly as an upload's
    // would be: a traversal here would read outside the storage root.
    validate_identifier("collection name", &collection).map_err(ApiError::new)?;
    let key = format!(
        "{collection}/{}/{}",
        safe_segment(&record_id)?,
        safe_segment(&filename)?
    );

    let storage = state.files.storage();

    let Some(preset) = preset else {
        let object = storage
            .get(&key)
            .await
            .map_err(ApiError::new)?
            .ok_or_else(|| ApiError::new(Error::not_found("no such file")))?;
        return Ok(image_response(object.body, &object.content_type));
    };

    let thumbnail_key = preset.thumbnail_key(&key);

    // A thumbnail is immutable — it is derived from an object whose key already
    // carries a random component — so it is generated once and then cached in the
    // same backend as the original.
    if let Some(cached) = storage.get(&thumbnail_key).await.map_err(ApiError::new)? {
        return Ok(image_response(cached.body, &cached.content_type));
    }

    let source = storage
        .get(&key)
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| ApiError::new(Error::not_found("no such file")))?;

    // The bytes decide the format, never a caller. A file that is not an image we
    // can decode gets a clear refusal rather than a broken thumbnail.
    let source_format = lazypock_files::sniff(&source.body).ok_or_else(|| {
        ApiError::new(Error::invalid(
            "this file is not an image that can be resized",
        ))
    })?;

    let bytes = thumbnail::resize(
        thumbnail::ResizeRequest {
            source: &source.body,
            source_format,
            width: preset.width,
            height: preset.height,
            mode: preset.mode,
            format: preset.format,
            quality: preset.quality,
        },
        // The configured image service, when there is one: the resize then happens
        // out of this process. A service that refuses the work is an error, never a
        // quiet fallback to the local codecs — the deployment asked for the CPU to
        // be elsewhere.
        state.files.image_service(),
    )
    .await
    .map_err(ApiError::new)?;

    let content_type = preset.format.content_type().to_string();
    storage
        .put(&thumbnail_key, &content_type, Bytes::from(bytes.clone()))
        .await
        .map_err(ApiError::new)?;

    tracing::debug!(key = %thumbnail_key, "generated a thumbnail");
    Ok(image_response(Bytes::from(bytes), &content_type))
}

/// A response for an image body.
///
/// Cacheable forever: an object at a key never changes, and a thumbnail is
/// derived from one.
fn image_response(body: Bytes, content_type: &str) -> Response {
    let mut response = (StatusCode::OK, body).into_response();
    let content_type = HeaderValue::from_str(content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    response
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

/// Whether a record row exists.
async fn record_exists(state: &AppState, collection: &str, record: &str) -> Result<bool, ApiError> {
    // Already validated as an identifier by the caller; quoted here so a name that
    // happens to be a keyword is still just a name.
    let sql = format!("select 1 from \"{collection}\" where id = $1::text");
    let client = state.db.client().await.map_err(ApiError::new)?;
    Ok(client
        .run_query_opt(&sql, &[&record.to_string()])
        .await
        .map_err(|err| ApiError::new(db_error(err)))?
        .is_some())
}

/// The body accepted by the sweep endpoint.
///
/// Unknown fields are rejected rather than ignored: a misspelled `dryRun` would
/// otherwise be dropped and the default used, which on this endpoint means the
/// difference between reporting and deleting.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweepBody {
    /// Report without deleting. Defaults to true, because the dangerous
    /// direction of this operation is deleting.
    #[serde(default = "default_true", rename = "dryRun")]
    pub dry_run: bool,
    /// Most objects to examine in one run.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_true() -> bool {
    true
}

fn default_limit() -> usize {
    10_000
}

/// `POST /api/files/sweep`
///
/// Reclaims stored objects whose owning record is gone. Superuser-only: it walks
/// and deletes across the whole store.
pub async fn sweep(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<SweepBody>,
) -> Result<Response, ApiError> {
    if !principal.is_superuser() {
        return Err(ApiError::new(Error::forbidden(
            "sweeping storage requires a superuser token",
        )));
    }

    let options = lazypock_files::SweepOptions {
        dry_run: body.dry_run,
        limit: body.limit,
    };
    let client = state.db.client().await.map_err(ApiError::new)?;
    let storage = state.files.storage();

    let report = lazypock_files::sweep::sweep(&client, storage.as_ref(), &options)
        .await
        .map_err(ApiError::new)?;

    // A dry run stops here. A real one also drains, so a single admin call leaves
    // the store consistent rather than waiting for a worker.
    let drained = if options.dry_run {
        outbox::DrainReport::default()
    } else {
        outbox::drain(&client, storage.as_ref())
            .await
            .map_err(ApiError::new)?
    };

    Ok(Json(json!({
        "dryRun": report.dry_run,
        "scanned": report.scanned,
        "orphans": report.orphans,
        "enqueued": report.enqueued,
        "deleted": drained.deleted,
        "failed": drained.failed,
        "unrecognised": report.unrecognised,
        "truncated": report.was_truncated(&options),
        "sample": report.sample,
    }))
    .into_response())
}

/// Query for the stored-file listing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    /// Only files belonging to this collection.
    #[serde(default)]
    pub collection: Option<String>,
    /// Only files belonging to this record.
    #[serde(default)]
    pub record: Option<String>,
    /// 1-based page.
    #[serde(default = "first_page")]
    pub page: usize,
    /// Items per page.
    #[serde(default = "default_per_page", rename = "perPage")]
    pub per_page: usize,
}

fn first_page() -> usize {
    1
}

fn default_per_page() -> usize {
    50
}

/// The largest page a caller may ask for.
const MAX_PER_PAGE: usize = 200;

/// `GET /api/files`
///
/// The media library has to be able to see what is stored, and nothing could: the
/// only way to reach an object was to already know its key. Keys are enumerated
/// from the backend itself rather than from a table, because there is no files
/// table — an object *is* its key.
///
/// Each item carries the URLs a client needs — the object itself, plus one per
/// preset — so a client renders from `url` rather than composing paths and
/// guessing the server's layout.
///
/// Superuser-only: this is the whole contents of the store.
pub async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    if !principal.is_superuser() {
        return Err(ApiError::new(Error::forbidden(
            "listing stored files requires a superuser token",
        )));
    }

    let keys = state
        .files
        .storage()
        .list("")
        .await
        .map_err(ApiError::new)?;

    // One query for the presets, so each item can offer its variants.
    let client = state.db.client().await.map_err(ApiError::new)?;
    let presets = lazypock_files::presets::load(&client)
        .await
        .map_err(ApiError::new)?;

    let mut items: Vec<Value> = Vec::new();
    for key in &keys {
        // Only source objects: a thumbnail is reached through its source's
        // `variants`, so listing it separately would show every image several
        // times over.
        let lazypock_files::sweep::ObjectOwner::Source { collection, record } =
            lazypock_files::sweep::classify(key)
        else {
            continue;
        };
        if query
            .collection
            .as_deref()
            .is_some_and(|wanted| wanted != collection)
        {
            continue;
        }
        if query
            .record
            .as_deref()
            .is_some_and(|wanted| wanted != record)
        {
            continue;
        }

        let name = key.rsplit('/').next().unwrap_or_default().to_string();
        let url = state.files.url(&collection, &record, &name);

        let mut variants = serde_json::Map::new();
        // `content` is the full-size object, which is the variant a rich text
        // editor wants; each preset adds its own named variant.
        variants.insert("content".to_string(), json!(url.clone()));
        for preset in &presets {
            variants.insert(
                preset.name.clone(),
                json!(format!("{url}?preset={}", preset.name)),
            );
        }

        items.push(json!({
            "collection": collection,
            "record": record,
            "name": name,
            "key": key,
            "url": url,
            "variants": Value::Object(variants),
        }));
    }

    items.sort_by(|left, right| {
        left["key"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["key"].as_str().unwrap_or_default())
    });

    let per_page = query.per_page.clamp(1, MAX_PER_PAGE);
    let page = query.page.max(1);
    let total = items.len();
    let total_pages = total.div_ceil(per_page).max(1);
    let start = (page - 1).saturating_mul(per_page).min(total);
    let window: Vec<Value> = items.into_iter().skip(start).take(per_page).collect();

    // PocketBase's list shape, which the clients already speak.
    Ok(Json(json!({
        "page": page,
        "perPage": per_page,
        "totalItems": total,
        "totalPages": total_pages,
        "items": window,
    })))
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
