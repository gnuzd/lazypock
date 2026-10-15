//! Collection administration.
//!
//! Every endpoint here is **superuser-only**, reads included. A collection's
//! definition carries its access rules, so listing them is not public
//! information; a client that needs to address a collection by name learns it from
//! the records API, which resolves names on its own.
//!
//! Changing a table's shape goes through the field endpoints rather than the
//! collection update, so the DDL that alters real tables stays in one place in the
//! data layer.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lazypock_core::error::Error;
use lazypock_db::collections::{
    self, Collection, CreateCollection, CreateField, Field, UpdateCollection,
};
use lazypock_db::types::{self, option_bool, CollectionType, FieldType};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::SystemTime;

use crate::auth::{require_superuser, require_superuser_or_api_key};
use crate::error::ApiError;
use crate::state::AppState;

/// `GET /api/collections`
pub async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    require_superuser_or_api_key(&principal)?;
    let client = state.db.client().await.map_err(ApiError::new)?;
    let collections = collections::list(&client).await.map_err(ApiError::new)?;

    // The system collections lead the list, which is where an `_`-prefixed name
    // sorts. They are synthesised, not stored: see `system_collections`.
    let system = system_collections();
    let mut items = Vec::with_capacity(collections.len() + system.len());
    for collection in &system {
        items.push(collection_json(collection, true)?);
    }
    for collection in &collections {
        items.push(collection_json(collection, is_system(&collection.name))?);
    }

    // The **list envelope**, not a bare array. A client reads `items`, so an array
    // arrives as an object with no `items` and the collection list renders empty —
    // which is exactly what happened: the SDK's `getList()` returned `null` and
    // Studio showed no collections at all, with a 200 in the network tab and nothing
    // to suggest why.
    let total = items.len();
    let per_page = query.per_page.unwrap_or(200).clamp(1, 200);
    let page = query.page.unwrap_or(1).max(1);
    let start = (page - 1).saturating_mul(per_page).min(total);
    let window: Vec<Value> = items.into_iter().skip(start).take(per_page).collect();

    Ok(Json(json!({
        "page": page,
        "perPage": per_page,
        "totalItems": total,
        "totalPages": total.div_ceil(per_page).max(1),
        "items": window,
    })))
}

/// Paging for the collections list, as the client sends it.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    /// 1-based page.
    #[serde(default)]
    pub page: Option<usize>,
    /// Items per page.
    #[serde(default, rename = "perPage")]
    pub per_page: Option<usize>,
}

/// `GET /api/collections/{name}`
pub async fn get(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_superuser_or_api_key(&principal)?;
    if let Some(collection) = system_collections().into_iter().find(|c| c.name == name) {
        return Ok(Json(collection_json(&collection, true)?));
    }
    let client = state.db.client().await.map_err(ApiError::new)?;
    let collection = collections::find(&client, &name)
        .await
        .map_err(ApiError::new)?
        .ok_or_else(|| not_found(&name))?;
    Ok(Json(collection_json(&collection, false)?))
}

/// The flat rule keys the Studio's collection editor sends as siblings of `name`.
const RULE_KEYS: &[&str] = &[
    "listRule",
    "viewRule",
    "createRule",
    "updateRule",
    "deleteRule",
    "manageRule",
];

/// Fold the flat rule keys the Studio sends into the nested `rules` object the data
/// layer reads, and refuse options the server cannot honour.
///
/// The Studio sends `listRule`, `viewRule`, … as siblings of `name`/`fields`, plus
/// `indexes`. The data layer reads `rules.listRule`, and serde silently dropped the
/// flat keys — so a rule set in the editor was never stored and every collection
/// stayed default-deny. Folding them in is the fix; refusing a non-empty `indexes`
/// list is the fail-loud half, because those would otherwise be silently ignored.
fn normalize_collection_body(body: Value) -> Result<Value, ApiError> {
    let Value::Object(mut object) = body else {
        return Err(ApiError::new(Error::invalid(
            "the collection must be a JSON object",
        )));
    };

    if object
        .get("indexes")
        .and_then(Value::as_array)
        .is_some_and(|indexes| !indexes.is_empty())
    {
        return Err(ApiError::new(Error::unsupported(
            "custom collection indexes are not supported yet; the collection was not saved \
             rather than stored without them",
        )));
    }
    object.remove("indexes");

    let mut rules = object
        .get("rules")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in RULE_KEYS {
        if let Some(value) = object.remove(*key) {
            rules.insert((*key).to_string(), value);
        }
    }
    if !rules.is_empty() {
        object.insert("rules".to_string(), Value::Object(rules));
    }

    Ok(Value::Object(object))
}

/// `POST /api/collections`
///
/// The body is the data layer's own input type, so the wire shape and the layer
/// that builds the DDL cannot drift apart. It is normalised first, to accept the
/// shape the Studio sends.
pub async fn create(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    require_superuser(&principal)?;

    let body: CreateCollection = serde_json::from_value(normalize_collection_body(body)?)
        .map_err(|err| ApiError::new(Error::invalid(format!("invalid collection: {err}"))))?;

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    let collection = collections::create(&mut client, body)
        .await
        .map_err(ApiError::new)?;

    tracing::info!(collection = %collection.name, "collection created");
    let body = collection_json(&collection, is_system(&collection.name))?;
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

/// `PATCH /api/collections/{name}`
pub async fn update(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    require_superuser(&principal)?;

    let body: UpdateCollection = serde_json::from_value(normalize_collection_body(body)?)
        .map_err(|err| ApiError::new(Error::invalid(format!("invalid collection: {err}"))))?;

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    // Confirm it exists first, so the error is a 404 rather than whatever the
    // update happens to report.
    if collections::find(&client, &name)
        .await
        .map_err(ApiError::new)?
        .is_none()
    {
        return Err(not_found(&name));
    }

    let collection = collections::update(&mut client, &name, body)
        .await
        .map_err(ApiError::new)?;
    tracing::info!(collection = %collection.name, "collection updated");
    Ok(Json(collection_json(
        &collection,
        is_system(&collection.name),
    )?))
}

/// Query for deleting a collection.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteQuery {
    /// Whether to drop the table and its rows.
    ///
    /// **Defaults to true**, matching `main` and PocketBase: deleting a collection
    /// deletes its table. Keeping the data is the explicit opt-out (`dropData=false`),
    /// because otherwise a delete left the table behind and re-creating the same name
    /// failed with "a table already exists".
    #[serde(default, rename = "dropData")]
    pub drop_data: Option<bool>,
}

/// `DELETE /api/collections/{name}`
pub async fn delete(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    require_superuser(&principal)?;

    let drop_data = query.drop_data.unwrap_or(true);
    let mut client = state.db.client().await.map_err(ApiError::new)?;
    if collections::find(&client, &name)
        .await
        .map_err(ApiError::new)?
        .is_none()
    {
        return Err(not_found(&name));
    }

    // A dropped collection's uploads go with it. The rows are removed first so each
    // object is queued by the `_files_deleted` trigger while the table still exists.
    lazypock_files::store::delete_for_collection(&client, &name)
        .await
        .map_err(ApiError::new)?;

    let collection = collections::delete(&mut client, &name, drop_data)
        .await
        .map_err(ApiError::new)?;

    tracing::warn!(collection = %name, drop_data, "collection deleted");
    Ok(Json(collection_json(
        &collection,
        is_system(&collection.name),
    )?))
}

/// `POST /api/collections/{name}/fields`
pub async fn add_field(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Json(body): Json<CreateField>,
) -> Result<Response, ApiError> {
    require_superuser(&principal)?;

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    let field = collections::add_field(&mut client, &name, body)
        .await
        .map_err(ApiError::new)?;

    tracing::info!(collection = %name, field = %field.name, "field added");
    Ok((StatusCode::CREATED, Json(field)).into_response())
}

/// `DELETE /api/collections/{name}/fields/{field}`
pub async fn delete_field(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((name, field)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    require_superuser(&principal)?;

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    collections::delete_field(&mut client, &name, &field)
        .await
        .map_err(ApiError::new)?;

    tracing::warn!(collection = %name, field = %field, "field deleted");
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The error for a collection that is not there.
///
/// One helper so every endpoint reports it the same way.
fn not_found(name: &str) -> ApiError {
    ApiError::new(Error::not_found(format!("no collection named {name:?}")))
}

/// The system collections the admin API presents, in list order.
///
/// They are synthesised **not** `_collections` rows, because that table feeds the
/// schema cache: an entry visible there would also be served by the records API and
/// would be reachable through the collection/field admin endpoints. Synthesising
/// keeps a system collection visible in the Studio's System section while leaving
/// it outside the schema cache — so it can be neither read as records nor altered.
///
/// The list is the PocketBase set. Four of these tables are not created yet —
/// OAuth2, MFA and OTP are Track E backlog (PLAN.md 7.1) — so today they are
/// presented for parity and have no records to serve.
fn system_collections() -> Vec<Collection> {
    vec![
        superusers_collection(),
        system_collection(
            "_auth_origins",
            &[
                ("collection_ref", FieldType::Text),
                ("record_ref", FieldType::Text),
                ("fingerprint", FieldType::Text),
                ("created_at", FieldType::Autodate),
                ("updated_at", FieldType::Autodate),
            ],
        ),
        system_collection(
            "_external_auths",
            &[
                ("collection", FieldType::Text),
                ("provider", FieldType::Text),
                ("provider_id", FieldType::Text),
                ("user_id", FieldType::Text),
                ("created_at", FieldType::Autodate),
                ("updated_at", FieldType::Autodate),
            ],
        ),
        system_collection(
            "_mfas",
            &[
                ("collection_ref", FieldType::Text),
                ("record_ref", FieldType::Text),
                ("method", FieldType::Text),
                ("created_at", FieldType::Autodate),
                ("updated_at", FieldType::Autodate),
            ],
        ),
        system_collection(
            "_otps",
            &[
                ("collection_ref", FieldType::Text),
                ("record_ref", FieldType::Text),
                ("password_hash", FieldType::Password),
                ("sent_to", FieldType::Text),
                ("created_at", FieldType::Autodate),
                ("updated_at", FieldType::Autodate),
            ],
        ),
    ]
}

/// Whether a collection is one of the server's own (`_`-prefixed) tables.
///
/// The same convention `reflect.rs` and `backup.rs` use. A user collection can
/// never match: `validate_collection_name` rejects a leading `_`.
fn is_system(name: &str) -> bool {
    name.starts_with('_')
}

/// The `_superusers` collection, presented to the Studio.
///
/// It is deliberately **not** a `_collections` row. It has no table-driven schema
/// of its own and the auth path special-cases it, but the reason it must stay out
/// of `collections::list` is sharper: that list feeds the schema cache, so a
/// `_superusers` visible there would make `records::resolve` serve superuser rows —
/// and the `_superusers` table has no row-level security. Synthesising it in the
/// admin list and single-read keeps it visible (and locked) without exposing it as
/// a records collection.
fn superusers_collection() -> Collection {
    let id = crate::auth::SUPERUSERS.to_string();
    let fields = types::auth_system_fields()
        .into_iter()
        .enumerate()
        .map(
            |(position, (name, field_type, required, is_unique, options))| Field {
                id: format!("{id}_{name}"),
                collection_id: id.clone(),
                name: name.to_string(),
                field_type,
                required,
                is_unique,
                options,
                position: position as i32,
            },
        )
        .collect();
    let now = SystemTime::now();
    Collection {
        id,
        name: crate::auth::SUPERUSERS.to_string(),
        collection_type: CollectionType::Auth,
        options: json!({}),
        rules: json!({}),
        fields,
        created: now,
        updated: now,
    }
}

/// A synthesised system collection from its name and column list.
///
/// The fields are metadata for the Studio (its editor and any column list); the
/// entry is not in the schema cache, so nothing here can be read as records or
/// altered through the admin endpoints.
fn system_collection(name: &str, columns: &[(&str, FieldType)]) -> Collection {
    let id = name.to_string();
    let now = SystemTime::now();
    Collection {
        id: id.clone(),
        name: name.to_string(),
        collection_type: CollectionType::Base,
        options: json!({}),
        rules: json!({}),
        fields: columns
            .iter()
            .enumerate()
            .map(|(position, (field, field_type))| Field {
                id: format!("{id}_{field}"),
                collection_id: id.clone(),
                name: field.to_string(),
                field_type: *field_type,
                required: false,
                is_unique: false,
                options: json!({}),
                position: position as i32,
            })
            .collect(),
        created: now,
        updated: now,
    }
}

/// Serialize a collection in the shape the Studio reads.
///
/// This mirrors what the Elixir implementation served, which the Studio and the
/// published import/export format were built against:
///
/// - `system` places an entry under the Studio's System section and locks it. It is
///   derived from the `_` prefix, the same convention the rest of the server uses.
/// - `schema` is the field list under its older spelling, which the Studio's sidebar
///   counts. It is derived from `fields` here so the two can never disagree.
/// - each field carries `hidden`/`system`/`indexed` at the top level, where the
///   Studio reads them. `hidden` is stored inside `options` (PocketBase's shape), and
///   `system` is derived from the auth collection's built-in columns.
/// - `indexes`/`viewQuery`/`viewOrigin`/`viewBuilder` are the top-level
///   conveniences `main` provided. Custom indexes and view collections are not
///   implemented here yet, so they stay empty/null rather than disappearing.
/// - `created`/`updated` are rendered as RFC 3339. Serde would otherwise render the
///   `SystemTime`s as `{secs_since_epoch, nanos_since_epoch}`, which is not what a
///   client parses.
fn collection_json(collection: &Collection, system: bool) -> Result<Value, ApiError> {
    let mut value = serde_json::to_value(collection).map_err(|err| {
        ApiError::new(Error::internal(format!(
            "cannot serialize the collection {:?}: {err}",
            collection.name
        )))
    })?;

    value["system"] = json!(system);

    let schema: Vec<Value> = collection
        .fields
        .iter()
        .map(|field| {
            json!({
                "name": field.name,
                "type": field.field_type.as_str(),
                "required": field.required,
                "unique": field.is_unique,
                "default": null,
                "options": field.options,
                "indexed": false,
            })
        })
        .collect();
    value["schema"] = json!(schema);

    if let Some(fields) = value.get_mut("fields").and_then(Value::as_array_mut) {
        for (entry, field) in fields.iter_mut().zip(&collection.fields) {
            let Some(object) = entry.as_object_mut() else {
                continue;
            };
            object.insert(
                "hidden".to_string(),
                json!(option_bool(&field.options, "hidden")),
            );
            object.insert(
                "system".to_string(),
                json!(is_auth_system_field(collection, &field.name)),
            );
            object.insert("indexed".to_string(), json!(false));
        }
    }

    value["indexes"] = collection
        .options
        .get("indexes")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let is_view = collection.collection_type == CollectionType::View;
    value["viewQuery"] = if is_view {
        collection
            .options
            .get("view_query")
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    value["viewOrigin"] = if is_view {
        collection
            .options
            .get("view_origin")
            .cloned()
            .unwrap_or_else(|| json!("sql"))
    } else {
        Value::Null
    };
    value["viewBuilder"] = if is_view {
        collection
            .options
            .get("view_builder")
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    value["created"] = json!(rfc3339(collection.created));
    value["updated"] = json!(rfc3339(collection.updated));

    Ok(value)
}

/// Whether a field is one of an auth collection's built-in columns.
fn is_auth_system_field(collection: &Collection, name: &str) -> bool {
    collection.collection_type.is_auth()
        && types::auth_system_fields()
            .iter()
            .any(|(field, ..)| *field == name)
}

/// A `SystemTime` as RFC 3339, in UTC.
fn rfc3339(time: SystemTime) -> String {
    let seconds = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    lazypock_db::backup::format_rfc3339(seconds)
}

use axum::Extension;

use crate::auth::Principal;
