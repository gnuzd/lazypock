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
use lazypock_db::tokio_postgres::types::ToSql;
use lazypock_db::types::validate_identifier;
use lazypock_files::rules::{shift_placeholders, FileRules};
use lazypock_files::storage::Files;
use lazypock_files::store::{self, NewFile, Origin};
use lazypock_files::{keys, outbox, presets, thumbnail, ImagePreset};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

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
/// Parts: `file` (required), and optionally `collection`/`collection_name`,
/// `record`/`record_id`, `field_name`, `origin` and `variants`. Both spellings are
/// accepted because the Studio and the SDK send the underscored ones while this route
/// originally spelled them `collection`/`record` — and a part this server ignores
/// looks to the client like an upload that vanished.
///
/// An upload with **no record** is the editor's and the media library's case: it is
/// stored and left `pending`, a record write attaches it, and one nobody attaches is
/// reclaimed after the configured TTL. The response is the row a client reads, and its
/// `id` is what goes into a file field.
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
    let mut field_name = String::new();
    let mut origin = String::new();
    let mut requested_variants: Vec<String> = Vec::new();
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
            // The first spelling wins when a client sends both, rather than the last
            // one quietly deciding which record the file belongs to.
            "collection" | "collection_name" => {
                if collection.is_none() {
                    collection = Some(field.text().await.map_err(part_error)?);
                }
            }
            "record" | "record_id" => {
                if record_id.is_none() {
                    record_id = Some(field.text().await.map_err(part_error)?);
                }
            }
            "field_name" => field_name = field.text().await.map_err(part_error)?,
            "origin" => origin = field.text().await.map_err(part_error)?,
            "variants" => {
                requested_variants = field
                    .text()
                    .await
                    .map_err(part_error)?
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "file" => {
                // Read these before the field is consumed by `bytes`.
                filename = field.file_name().map(str::to_string);
                declared_type = field.content_type().unwrap_or_default().to_string();
                body = Some(field.bytes().await.map_err(part_error)?);
            }
            // Unknown parts are ignored rather than fatal: a client may send more
            // than we need.
            _ => {}
        }
    }

    let filename = filename.unwrap_or_else(|| "upload".to_string());
    let body = body.ok_or_else(|| ApiError::new(Error::invalid("a `file` part is required")))?;
    let origin = Origin::parse(&origin).map_err(ApiError::new)?;

    let collection = collection.unwrap_or_default().trim().to_string();
    let record_id = record_id.unwrap_or_default().trim().to_string();

    // A named collection has to exist, so an upload cannot create an object that
    // nothing could ever reference.
    if !collection.is_empty() && state.schema.snapshot().get(&collection).is_none() {
        return Err(ApiError::new(Error::not_found(format!(
            "no collection named {collection:?}"
        ))));
    }

    // A named record has to exist. The reaper reclaims an object whose record is gone,
    // so without this an upload for a record that is not there yet would be collected
    // before anything referenced it.
    if !record_id.is_empty() {
        if collection.is_empty() {
            return Err(ApiError::new(Error::invalid(
                "a `record` without a `collection` does not say where the file belongs",
            )));
        }
        if !record_exists(&state, &collection, &record_id).await? {
            return Err(ApiError::new(Error::not_found(format!(
                "no record {record_id:?} in the collection {collection:?}"
            ))));
        }
    }

    // Under the record when there is one — the shape the key-based sweeper attributes
    // — and in the library when there is not.
    let key = if collection.is_empty() || record_id.is_empty() {
        keys::pending_key(&filename)
    } else {
        keys::storage_key(&collection, &record_id, &filename).map_err(ApiError::new)?
    };

    // What the bytes say, read before they are handed over: the sniffer decides the
    // type, never the caller's claim, and the dimensions come from the image.
    // Validated here rather than only inside `store`, because the format and the
    // dimensions are needed for the row. The same validator is used either way, so the
    // refusal a caller sees (SVG, an unsupported type, an oversized upload) does not
    // depend on which layer noticed.
    let format = lazypock_files::validation::validate(&body, &declared_type, state.files.limits())
        .map_err(ApiError::new)?;
    let dimensions = lazypock_files::validation::dimensions(&body, format);
    let declared_size = body.len() as i64;
    let checksum = hex(&Sha256::digest(&body));
    let stored_name = key.rsplit('/').next().unwrap_or_default().to_string();

    state
        .files
        .store(&key, &declared_type, body)
        .await
        .map_err(ApiError::new)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    let mut file = store::insert(
        &client,
        &NewFile {
            filename: stored_name,
            extension: format.as_str().to_string(),
            mime_type: format.content_type().to_string(),
            size: declared_size,
            storage_path: key.clone(),
            storage_backend: state.files.backend_name().to_string(),
            collection_name: collection.clone(),
            record_id: record_id.clone(),
            field_name: field_name.clone(),
            origin,
            uploaded_by: principal.user_id().unwrap_or_default().to_string(),
            original_name: Some(filename.clone()),
            width: dimensions.map(|(width, _)| width as i32),
            height: dimensions.map(|(_, height)| height as i32),
            checksum: Some(checksum),
        },
    )
    .await
    .map_err(ApiError::new)?;

    // `variants=content,thumb` asks for those presets to exist *now*, which is the
    // difference between an editor showing a thumbnail immediately and showing one
    // after its first request. The keys are remembered on the row so the reaper can
    // remove them with the source.
    if !requested_variants.is_empty() {
        let mut generated = file.variants.clone();
        for name in &requested_variants {
            let preset = presets::find(&client, name)
                .await
                .map_err(ApiError::new)?
                .ok_or_else(|| {
                    ApiError::new(Error::not_found(format!("no image preset named {name:?}")))
                })?;
            variant_bytes(&state, &file.storage_path, &preset).await?;
            if let Some(object) = generated.as_object_mut() {
                object.insert(
                    preset.name.clone(),
                    json!(preset.thumbnail_key(&file.storage_path)),
                );
            }
        }
        store::set_variants(&client, &file.id, &generated)
            .await
            .map_err(ApiError::new)?;
        file.variants = generated;
    }

    let presets = presets::load(&client).await.map_err(ApiError::new)?;
    let body = file_json_with(&file, &state.files, &presets);
    tracing::info!(
        id = %file.id,
        key = %key,
        origin = %file.origin,
        "file stored"
    );

    Ok((StatusCode::CREATED, Json(body)).into_response())
}

/// The JSON a client reads for one file.
///
/// Both spellings appear where the SDK's TypeScript type and the Elixir-era API
/// disagreed (`mime_type`/`mimeType`, `uploaded_by`/`uploadedBy`): the Studio reads the
/// snake_case fields and the SDK's type names the camelCase ones, and a field a client
/// cannot find is one it silently renders as empty.
fn file_json_with(file: &store::FileRow, files: &Files, presets: &[ImagePreset]) -> Value {
    let url = files.file_url(&file.id);

    // `content` is the full-size object, which is the variant a rich-text editor
    // wants; each preset adds its own named variant, generated on first request.
    let mut variants = serde_json::Map::new();
    variants.insert("content".to_string(), json!(url.clone()));
    for preset in presets {
        variants.insert(
            preset.name.clone(),
            json!(format!("{url}?preset={}", preset.name)),
        );
    }

    json!({
        "id": file.id,
        "filename": file.filename,
        "extension": file.extension,
        // `type` is the name the Elixir-era API used for the format, and the Studio
        // reads it for its icons; `extension` and `mime_type` say the same thing in the
        // shapes the newer clients expect.
        "type": file.extension,
        "mime_type": file.mime_type,
        "mimeType": file.mime_type,
        "size": file.size,
        "url": url,
        "key": file.storage_path,
        "collection_name": file.collection_name,
        "record_id": file.record_id,
        // The names the storage-listing shape used, so a client written against it
        // keeps finding what it looks for.
        "collection": file.collection_name,
        "record": file.record_id,
        "name": file.filename,
        "field_name": file.field_name,
        "origin": file.origin,
        "status": file.status,
        "width": file.width,
        "height": file.height,
        "checksum": file.checksum,
        "uploaded_by": file.uploaded_by,
        "uploadedBy": file.uploaded_by,
        "variants": Value::Object(variants),
        "created_at": file.created_at,
        "updated_at": file.updated_at,
        "attached_at": file.attached_at,
    })
}

/// The bytes and content type of one variant, generating and caching it if needed.
///
/// One implementation for every path that needs a resized image: the key-addressed
/// route, the id-addressed route, and an upload that asked for its variants eagerly.
async fn variant_bytes(
    state: &AppState,
    key: &str,
    preset: &ImagePreset,
) -> Result<(Bytes, String), ApiError> {
    let storage = state.files.storage();
    let thumbnail_key = preset.thumbnail_key(key);

    // A variant is immutable — it is derived from an object whose key already carries
    // a random component — so it is generated once and then cached in the same backend
    // as the original.
    if let Some(cached) = storage.get(&thumbnail_key).await.map_err(ApiError::new)? {
        return Ok((cached.body, cached.content_type));
    }

    let source = storage
        .get(key)
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| ApiError::new(Error::not_found("no such file")))?;

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
        // The configured image service, when there is one: the resize then happens out
        // of this process. A service that refuses the work is an error, never a quiet
        // fallback to the local codecs — the deployment asked for the CPU to be
        // elsewhere.
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
    Ok((Bytes::from(bytes), content_type))
}

/// A lowercase hex digest, for the checksum column.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// `GET /api/files/{id}`
///
/// The id-addressed way to a file: what a client holding a file field's value uses.
/// `?preset=<name>` and `?thumb=<WxH|name>` select a variant, which is the pair the
/// SDK's URL helpers build.
pub async fn serve_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ServeQuery>,
) -> Result<Response, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let file = store::get(&client, &id)
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| ApiError::new(Error::not_found(format!("no file {id:?}"))))?;

    if query.thumb.is_some() && query.preset.is_some() {
        return Err(ApiError::new(Error::invalid(
            "give either `thumb` or `preset`, not both",
        )));
    }

    let specification = query.thumb.clone().or_else(|| query.preset.clone());
    let Some(specification) = specification else {
        let object = state
            .files
            .storage()
            .get(&file.storage_path)
            .await
            .map_err(ApiError::new)?
            .ok_or_else(|| ApiError::new(Error::not_found("no such file")))?;
        return Ok(image_response(object.body, &object.content_type));
    };

    let (bytes, content_type) = variant_of(&state, &file.storage_path, &specification).await?;
    remember_variant(&client, &file, &specification).await?;
    Ok(image_response(bytes, &content_type))
}

/// Record a generated variant on its row, so the reaper removes it with the source.
///
/// Without this a lazily generated thumbnail is an object nothing knows about: the row
/// is deleted, the source object is queued, and the thumbnail stays in the bucket. The
/// key-addressed route has no row to record it on — its objects are attributed by the
/// orphan sweeper instead.
async fn remember_variant(
    client: &lazypock_db::deadpool_postgres::Client,
    file: &store::FileRow,
    specification: &str,
) -> Result<(), ApiError> {
    let Ok(preset) = variant_preset(client, specification).await else {
        // The variant was served, so this cannot be a resolution failure; a missing
        // preset here means something else went wrong, and that is not this function's
        // to report.
        return Ok(());
    };

    let key = preset.thumbnail_key(&file.storage_path);
    let mut variants = file.variants.clone();
    let Some(object) = variants.as_object_mut() else {
        return Ok(());
    };
    if object.get(&preset.name).and_then(Value::as_str) == Some(key.as_str()) {
        return Ok(());
    }

    object.insert(preset.name.clone(), json!(key));
    store::set_variants(client, &file.id, &variants)
        .await
        .map_err(ApiError::new)?;
    Ok(())
}

/// The bytes of one variant, resolving `size` the way a client means it.
async fn variant_of(
    state: &AppState,
    key: &str,
    specification: &str,
) -> Result<(Bytes, String), ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let preset = variant_preset(&client, specification).await?;
    variant_bytes(state, key, &preset).await
}

/// Resolve a `size` the way a client means it.
///
/// A box (`400x300`) is an ad-hoc size; anything else names a preset, which is the rule
/// the SDK's `getScaleUrl` uses when it decides which parameter to send.
async fn variant_preset(
    client: &lazypock_db::deadpool_postgres::Client,
    specification: &str,
) -> Result<ImagePreset, ApiError> {
    if specification.contains('x') {
        return ImagePreset::parse_size(specification).map_err(ApiError::new);
    }

    presets::find(client, specification)
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| {
            ApiError::new(Error::not_found(format!(
                "no image preset named {specification:?}"
            )))
        })
}

/// One variant of a file addressed by id: shared by the query and the path spellings.
async fn serve_variant_by_id(
    state: AppState,
    id: &str,
    specification: &str,
) -> Result<Response, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let file = store::get(&client, id)
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| ApiError::new(Error::not_found(format!("no file {id:?}"))))?;

    let (bytes, content_type) = variant_of(&state, &file.storage_path, specification).await?;
    Ok(image_response(bytes, &content_type))
}

/// `GET /api/files/{id}/thumbs/{size}` and `GET /api/files/{id}/scale/{size}`
///
/// The path-shaped variants of the same request: the SDK's `thumbs`/`variants` maps
/// document these URLs, and the Elixir-era API served them, so a client that was
/// written against either keeps working. `{size}` is a preset name or a `WxH` box.
pub async fn serve_thumbnail_by_id(
    State(state): State<AppState>,
    Path((id, size)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    serve_variant_by_id(state, &id, &size).await
}

/// `GET /api/files/{id}/scale/{size}` — the same resolution as the thumb route.
pub async fn serve_scale_by_id(
    State(state): State<AppState>,
    Path((id, size)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    serve_variant_by_id(state, &id, &size).await
}

/// The query a delete accepts.
#[derive(Debug, Default, Deserialize)]
pub struct DeleteQuery {
    /// Delete even while a record still references the file. Superuser-only.
    #[serde(default)]
    pub force: bool,
}

/// `DELETE /api/files/{id}`
///
/// The row goes and the `_files_deleted` trigger queues the object, so the storage
/// delete happens after the commit even if the process dies here.
///
/// Two guards, both of which the Elixir server had and this one did not:
/// **a file a record still references is refused** with the list of references unless
/// a superuser passes `?force=true` — deleting it would leave that record pointing at
/// nothing — and a caller who is neither a superuser nor the uploader may not delete
/// somebody else's file.
pub async fn delete_by_id(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<Response, ApiError> {
    let Some(user_id) = principal.user_id() else {
        return Err(ApiError::new(Error::forbidden(
            "deleting a file requires an authenticated request",
        )));
    };

    let client = state.db.client().await.map_err(ApiError::new)?;
    let file = store::get(&client, &id)
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| ApiError::new(Error::not_found(format!("no file {id:?}"))))?;

    // `files.rules.deleteRule` decides who may: a filter is asked about *this* file, the
    // empty string opens it, and an absent rule leaves it to superusers. A superuser
    // bypasses it, and every unreadable rule denies.
    if !principal.is_superuser() {
        let rules = file_rules(&client).await?;
        match rules.delete_predicate(&principal.auth_context()) {
            Ok(None) => {}
            Ok(Some(predicate)) => {
                let sql = format!(
                    "select 1 from _files where id = $1::text and ({})",
                    shift_placeholders(&predicate.sql, 1)
                );
                let mut parameters: Vec<&(dyn ToSql + Sync)> = vec![&id];
                for value in &predicate.params {
                    parameters.push(value);
                }
                let allowed = client
                    .run_query_opt(&sql, &parameters)
                    .await
                    .map_err(|err| ApiError::new(db_error(err)))?
                    .is_some();
                if !allowed {
                    tracing::info!(
                        id = %id,
                        user = %user_id,
                        "file deleteRule does not match this file"
                    );
                    return Err(ApiError::new(Error::forbidden(
                        "Access denied by file deleteRule",
                    )));
                }
            }
            Err(err) => return Err(ApiError::new(err)),
        }
    }

    let usage = store::refs_for_file(&client, &id)
        .await
        .map_err(ApiError::new)?;
    if !usage.is_empty() && !(query.force && principal.is_superuser()) {
        // A 409 with a body, not an error: the Studio renders exactly this list and
        // asks whether to break the reference.
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({
                "status": 409,
                "message": "this file is still referenced by records; pass ?force=true to delete it anyway",
                "data": { "usage": usage },
            })),
        )
            .into_response());
    }

    store::delete(&client, &id).await.map_err(ApiError::new)?;
    tracing::info!(
        id = %id,
        key = %file.storage_path,
        forced = query.force,
        "file deleted"
    );
    Ok(StatusCode::NO_CONTENT.into_response())
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

    let (bytes, content_type) = variant_bytes(&state, &key, &preset).await?;
    Ok(image_response(bytes, &content_type))
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

/// The file rules, read from the `files` setting the Studio's Media page writes.
///
/// Read per request rather than cached: a settings change takes effect on the next
/// request, and a rule that is enforced one request late is a rule nobody can reason
/// about.
async fn file_rules(
    client: &lazypock_db::deadpool_postgres::Client,
) -> Result<FileRules, ApiError> {
    let row = client
        .query_opt("select value from _settings where key = 'files'", &[])
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;
    let setting = row.map(|row| row.get::<_, Value>(0)).unwrap_or(Value::Null);
    Ok(FileRules::from_setting(&setting))
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
    /// Only files belonging to this collection. `collectionName` is the SDK's name for
    /// the same filter.
    #[serde(default, alias = "collectionName")]
    pub collection: Option<String>,
    /// Only files belonging to this record.
    #[serde(default, alias = "recordId")]
    pub record: Option<String>,
    /// Only files in this field.
    #[serde(default, alias = "fieldName")]
    pub field: Option<String>,
    /// Only this content type, or this prefix (`image/`) — the Studio's images-only
    /// toggle sends the prefix.
    #[serde(default)]
    pub mime: Option<String>,
    /// Only names containing this text (`q` is what the Studio's search box sends).
    #[serde(default, alias = "q", alias = "search")]
    pub search: Option<String>,
    /// 1-based page.
    #[serde(default = "first_page")]
    pub page: i64,
    /// Items per page.
    #[serde(default = "default_per_page", rename = "perPage")]
    pub per_page: i64,
}

fn first_page() -> i64 {
    1
}

fn default_per_page() -> i64 {
    50
}

/// The largest page a caller may ask for.
const MAX_PER_PAGE: i64 = 200;

/// `GET /api/files`
///
/// The media library, served from `_files`. An upload that is not attached to a record
/// yet belongs here too — which a listing of the storage backend could not express,
/// since such an object has no record to be filed under and no size or uploader to
/// show. Each item is the shape an upload returns ([`file_json_with`]), so a client
/// renders the library and a fresh upload with one code path.
///
/// Both spellings of the envelope are included (`total` and `totalItems`): the SDK's
/// type says `total`, PocketBase's lists say `totalItems`, and the Studio falls back to
/// the page length when it finds neither — silently paginating to a single page.
///
/// `files.rules.listRule` decides who may: a filter narrows the listing to the rows it
/// matches, the empty string opens it, and an absent rule leaves it to superusers. A
/// superuser always sees everything, which is what keeps the Studio's media library
/// working once an operator has locked the API down.
pub async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let per_page = query.per_page.clamp(1, MAX_PER_PAGE);
    let page = query.page.max(1);

    let client = state.db.client().await.map_err(ApiError::new)?;
    let rules = file_rules(&client).await?;
    let restriction = match rules.list_predicate(&principal.auth_context()) {
        Ok(predicate) => predicate,
        // A superuser bypasses the rule rather than being exempted from reading it: an
        // unreadable rule must not stop the operator who has to fix it.
        Err(err) if principal.is_superuser() => {
            tracing::debug!(error = %err, "file listRule does not allow this request; superuser");
            None
        }
        Err(err) => return Err(ApiError::new(err)),
    };
    let restriction = if principal.is_superuser() {
        None
    } else {
        restriction
    };

    let (files, total) = store::list(
        &client,
        &store::ListQuery {
            page,
            per_page,
            collection: query.collection.clone(),
            record: query.record.clone(),
            field: query.field.clone(),
            mime: query.mime.clone(),
            search: query.search.clone(),
            origin: None,
            status: None,
            uploaded_by: None,
            restriction,
        },
    )
    .await
    .map_err(ApiError::new)?;

    // One query for the presets, so every item can offer its variants.
    let presets = presets::load(&client).await.map_err(ApiError::new)?;
    let items: Vec<Value> = files
        .iter()
        .map(|file| file_json_with(file, &state.files, &presets))
        .collect();

    Ok(Json(json!({
        "items": items,
        "page": page,
        "perPage": per_page,
        "total": total,
        "totalItems": total,
        "totalPages": total.div_euclid(per_page).max(1),
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
