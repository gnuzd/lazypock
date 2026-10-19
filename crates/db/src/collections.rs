//! Collections and fields: the persisted model and its repository.
//!
//! A collection is one real Postgres table; its metadata lives in `_collections`
//! and `_fields`. Metadata changes and their DDL happen in a **single
//! transaction** — Postgres DDL is transactional, so a failure leaves neither a
//! stray table nor an orphaned metadata row — and every mutation emits
//! `NOTIFY lazypock_schema_changed` so other nodes reload their schema cache.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use lazypock_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio_postgres::Client;

use crate::ddl;
use crate::indexes;
use crate::pool::db_error;
use crate::query::compile::FilterContext;
use crate::rls;
use crate::types::{
    self, validate_collection_name, validate_field_name, validate_options, CollectionType,
    FieldType,
};

/// Channel other nodes listen on to invalidate their schema cache.
pub const SCHEMA_CHANGED_CHANNEL: &str = "lazypock_schema_changed";

/// A collection as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    /// 15-character record id.
    pub id: String,
    /// Lowercase snake_case name, also the table/view name.
    pub name: String,
    /// Collection kind.
    #[serde(rename = "type")]
    pub collection_type: CollectionType,
    /// Type-specific settings (view SQL, auth options, ...).
    #[serde(default)]
    pub options: Value,
    /// Access rules keyed by action (`listRule`, `viewRule`, ...).
    #[serde(default)]
    pub rules: Value,
    /// User-defined fields, in display order.
    #[serde(default)]
    pub fields: Vec<Field>,
    /// Creation time.
    pub created: SystemTime,
    /// Last metadata change.
    pub updated: SystemTime,
}

impl Collection {
    /// Validate the collection and all of its fields.
    pub fn validate(&self) -> Result<()> {
        validate_collection_name(&self.name)?;

        let mut seen: HashSet<&str> = HashSet::new();
        for field in &self.fields {
            if !seen.insert(field.name.as_str()) {
                return Err(Error::invalid(format!(
                    "duplicate field {:?} in collection {:?}",
                    field.name, self.name
                )));
            }
            field.validate(self.collection_type)?;
        }
        Ok(())
    }

    /// Field lookup by name.
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }

    /// The collection's password field, when it has one.
    ///
    /// An auth collection always does — `password_hash` — and its name is what the
    /// Studio's generated form sends a password under, so the password write path
    /// resolves the column through here rather than assuming the name.
    pub fn password_field(&self) -> Option<&Field> {
        self.fields
            .iter()
            .find(|field| field.field_type == FieldType::Password)
    }

    /// The password value in an incoming body, under either spelling.
    ///
    /// PocketBase's canonical write-only key is `password`; the Studio's form is
    /// generated from the collection's fields, so it sends the field's own name
    /// (`password_hash`). `main` accepted both, so accepting only the canonical one
    /// made a Studio-created account fail with "A password is required".
    pub fn password_input<'a>(&self, input: &'a Map<String, Value>) -> Option<&'a Value> {
        let field = self.password_field()?;
        input.get("password").or_else(|| input.get(&field.name))
    }
}

/// A field as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// 15-character record id.
    pub id: String,
    /// Owning collection id.
    #[serde(rename = "collectionId")]
    pub collection_id: String,
    /// Lowercase snake_case column name.
    pub name: String,
    /// Field type.
    #[serde(rename = "type")]
    pub field_type: FieldType,
    /// Whether the column is `not null`.
    pub required: bool,
    /// Whether a unique index is maintained.
    #[serde(rename = "unique")]
    pub is_unique: bool,
    /// Type-specific settings.
    #[serde(default)]
    pub options: Value,
    /// Display position.
    ///
    /// The wire name is `sort_order`, which is what the Studio and the SDK read and
    /// write; `position` stays the column name. It used to be serialized as
    /// `position`, which neither client looked at, so a field list always rendered
    /// in its stored order no matter what the editor did.
    #[serde(rename = "sort_order")]
    pub position: i32,
}

impl Field {
    /// Postgres column type for this field.
    pub fn column_type(&self) -> Result<&'static str> {
        types::column_type(self.field_type, &self.options)
    }

    /// Whether the field stores multiple values.
    pub fn is_multiple(&self) -> bool {
        types::is_multiple(&self.options)
    }

    /// Whether the field asks for a plain (non-unique) index.
    ///
    /// The field-level `indexed` flag is part of the documented import format and of
    /// the published JSON Schema. It was stored and never used, so the collection's
    /// fields each read as indexed and none of them were.
    pub fn is_indexed(&self) -> bool {
        types::option_bool(&self.options, "indexed")
    }

    /// The default this field declares, if any.
    ///
    /// A reflected field's `default` is Postgres's own SQL text, which the database
    /// already applies; it is not a declarable default and is not reported as one.
    pub fn default_value(&self) -> Option<&Value> {
        if self.options.get("reflected").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        self.options.get("default").filter(|value| !value.is_null())
    }

    /// Validate the field name and options for its collection type.
    pub fn validate(&self, collection_type: CollectionType) -> Result<()> {
        validate_field_name(&self.name, collection_type)?;
        if self.field_type == FieldType::Password {
            return Err(Error::unsupported(
                "`password` is a built-in auth column (password_hash) and cannot be defined as an \
                 explicit field",
            ));
        }
        validate_options(self.field_type, &self.options)?;
        Ok(())
    }
}

/// Input for creating a collection.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCollection {
    /// Collection name.
    pub name: String,
    /// Collection kind; defaults to `base`.
    #[serde(rename = "type", default = "default_collection_type")]
    pub collection_type: CollectionType,
    /// Fields to create.
    #[serde(default)]
    pub fields: Vec<CreateField>,
    /// Access rules.
    #[serde(default = "empty_object")]
    pub rules: Value,
    /// Type-specific settings.
    #[serde(default = "empty_object")]
    pub options: Value,
    /// Attach to a pre-existing table of the same name instead of failing.
    ///
    /// This is the recovery path after [`delete`] with `drop_data = false`, which
    /// removes the metadata but keeps the data. Off by default so an unrelated or
    /// hand-made table is never claimed silently.
    #[serde(default)]
    pub adopt_existing_table: bool,
}

/// Input for creating or adding a field.
/// Deserialisation is hand-written below, because a field arrives in one of two
/// shapes; the `serde` attributes would belong to a derive that is no longer there.
#[derive(Debug, Clone)]
pub struct CreateField {
    /// Field name.
    pub name: String,
    /// Field type, `type` on the wire.
    pub field_type: FieldType,
    /// Whether the column is required.
    pub required: bool,
    /// Whether the column is unique.
    pub unique: bool,
    /// Type-specific settings, also accepted at the field's top level.
    pub options: Value,
}

/// Accept a field in **either** shape.
///
/// PocketBase — and therefore Studio — puts a field type's settings at the field's
/// top level: `{ "name": "author", "type": "relation", "collectionId": "…" }`. This
/// server's own format nests them under `options`. Serde drops unknown keys, so
/// before this the flat shape arrived with its settings silently discarded and
/// validation then reported the option missing — "a relation field needs a
/// `collectionId` option" — which named the symptom and hid the cause. Both shapes are now merged, with the
/// nested `options` winning where they overlap, and it applies to every field type's
/// settings — `values` for a select, presets for a file — not only relations.
impl<'de> Deserialize<'de> for CreateField {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Flat {
            name: String,
            #[serde(rename = "type")]
            field_type: FieldType,
            #[serde(default)]
            required: bool,
            #[serde(default)]
            unique: bool,
            #[serde(default)]
            options: Option<serde_json::Map<String, Value>>,
            /// Everything else, which is the type's settings in PocketBase's shape.
            #[serde(flatten)]
            settings: serde_json::Map<String, Value>,
        }

        let flat = Flat::deserialize(deserializer)?;

        // Flattened first, so an explicit `options` entry wins over a top-level one.
        let mut merged = flat.settings;
        if let Some(options) = flat.options {
            for (key, value) in options {
                merged.insert(key, value);
            }
        }

        Ok(CreateField {
            name: flat.name,
            field_type: flat.field_type,
            required: flat.required,
            unique: flat.unique,
            options: Value::Object(merged),
        })
    }
}

fn default_collection_type() -> CollectionType {
    CollectionType::Base
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// List every collection with its fields, ordered by name.
pub async fn list(client: &Client) -> Result<Vec<Collection>> {
    let rows = client
        .query(
            "select id, name, type, options, rules, created, updated \
             from _collections order by name",
            &[],
        )
        .await
        .map_err(db_error)?;

    let mut collections = Vec::with_capacity(rows.len());
    for row in &rows {
        collections.push(Collection {
            id: row.get("id"),
            name: row.get("name"),
            collection_type: CollectionType::parse(&row.get::<_, String>("type"))?,
            options: row.get("options"),
            rules: row.get("rules"),
            fields: Vec::new(),
            created: row.get("created"),
            updated: row.get("updated"),
        });
    }

    let field_rows = client
        .query(
            "select id, collection_id, name, type, required, is_unique, options, position \
             from _fields order by collection_id, position, name",
            &[],
        )
        .await
        .map_err(db_error)?;

    let mut index: HashMap<String, usize> = HashMap::with_capacity(collections.len());
    for (position, collection) in collections.iter().enumerate() {
        index.insert(collection.id.clone(), position);
    }

    for row in &field_rows {
        let collection_id: String = row.get("collection_id");
        let Some(&position) = index.get(&collection_id) else {
            return Err(Error::internal(format!(
                "field {} references unknown collection {collection_id}",
                row.get::<_, String>("id")
            )));
        };
        let field = Field {
            id: row.get("id"),
            collection_id,
            name: row.get("name"),
            field_type: FieldType::parse(&row.get::<_, String>("type"))?,
            required: row.get("required"),
            is_unique: row.get("is_unique"),
            options: row.get("options"),
            position: row.get("position"),
        };
        collections[position].fields.push(field);
    }

    // A view's fields are its columns, which come from the query rather than from
    // `_fields`: deriving them here is what keeps a redefined view — one with a new
    // column — readable without a metadata edit.
    load_view_fields(client, &mut collections).await?;

    Ok(collections)
}

/// Fill in the fields of every view collection from the catalog.
async fn load_view_fields(client: &Client, collections: &mut [Collection]) -> Result<()> {
    if !collections
        .iter()
        .any(|collection| collection.collection_type == CollectionType::View)
    {
        return Ok(());
    }

    let rows = client
        .query(
            "select c.relname as view_name, a.attname as name, t.typname as data_type, \
                    a.attnum::int as position \
               from pg_attribute a \
               join pg_class c on c.oid = a.attrelid \
               join pg_type t on t.oid = a.atttypid \
               join pg_namespace n on n.oid = c.relnamespace \
              where c.relkind = 'v' \
                and n.nspname = current_schema() \
                and a.attnum > 0 and not a.attisdropped \
              order by c.relname, a.attnum",
            &[],
        )
        .await
        .map_err(db_error)?;

    let mut by_view: HashMap<String, Vec<Field>> = HashMap::new();
    for row in &rows {
        let view_name: String = row.get("view_name");
        let field = view_field(
            &view_name,
            "",
            &row.get::<_, String>("name"),
            &row.get::<_, String>("data_type"),
            row.get("position"),
        );
        by_view.entry(view_name).or_default().push(field);
    }

    for collection in collections.iter_mut() {
        if collection.collection_type != CollectionType::View {
            continue;
        }
        if let Some(fields) = by_view.remove(&collection.name) {
            collection.fields = fields
                .into_iter()
                .map(|mut field| {
                    field.collection_id = collection.id.clone();
                    field
                })
                .collect();
        }
    }

    Ok(())
}

/// One field from a view column.
///
/// The mapping is the reflection mapping, on purpose: a view's column is the same
/// kind of thing as an adopted table's column, and a second mapper would be a second
/// set of rules for the same problem.
fn view_field(
    view: &str,
    collection_id: &str,
    name: &str,
    data_type: &str,
    position: i32,
) -> Field {
    let column = crate::reflect::Column {
        table: view.to_string(),
        name: name.to_string(),
        data_type: data_type.to_string(),
        type_oid: 0,
        not_null: false,
        is_primary: name == "id",
        is_unique: false,
        position,
        default: None,
    };
    let (field_type, options) = crate::reflect::map_column(&column, None);
    Field {
        id: lazypock_core::id::new_id(),
        collection_id: collection_id.to_string(),
        name: name.to_string(),
        field_type,
        required: false,
        is_unique: false,
        options,
        position,
    }
}

/// The fields a view exposes, read inside the caller's transaction.
async fn view_fields(
    runner: &dyn crate::runner::QueryRunner,
    view: &str,
    collection_id: &str,
) -> Result<Vec<Field>> {
    let rows = runner
        .run_query(
            "select a.attname as name, t.typname as data_type, a.attnum::int as position \
               from pg_attribute a \
               join pg_class c on c.oid = a.attrelid \
               join pg_type t on t.oid = a.atttypid \
               join pg_namespace n on n.oid = c.relnamespace \
              where c.relkind = 'v' \
                and n.nspname = current_schema() \
                and c.relname = $1 \
                and a.attnum > 0 and not a.attisdropped \
              order by a.attnum",
            &[&view.to_string()],
        )
        .await
        .map_err(db_error)?;

    Ok(rows
        .iter()
        .map(|row| {
            view_field(
                view,
                collection_id,
                &row.get::<_, String>("name"),
                &row.get::<_, String>("data_type"),
                row.get("position"),
            )
        })
        .collect())
}

/// Resolve a view collection's `options` into the SQL Postgres will run.
///
/// Two spellings, one meaning: `view_query` is the SQL, `view_builder` is the no-code
/// spec it is generated from. The generated SQL is written back into `view_query` — the
/// builder module's contract — so everything downstream (preview, import, export) sees
/// one definition.
fn view_options(options: &Value, all: &[Collection]) -> Result<Value> {
    let mut normalized = options.as_object().cloned().unwrap_or_default();

    if let Some(indexes) = normalized.get("indexes").and_then(Value::as_array) {
        if !indexes.is_empty() {
            return Err(Error::unsupported(
                "a view collection cannot have indexes; the source tables' indexes are what a view uses",
            ));
        }
    }

    match normalized
        .get("view_builder")
        .filter(|value| !value.is_null())
    {
        Some(spec) => {
            let query = crate::view_builder::to_query(spec, all)?;
            normalized.insert("view_query".to_string(), Value::String(query));
            normalized.insert(
                "view_origin".to_string(),
                Value::String("builder".to_string()),
            );
        }
        None => {
            let query = normalized
                .get("view_query")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|query| !query.is_empty())
                .ok_or_else(|| {
                    Error::invalid(
                        "a view collection needs `viewQuery` (SQL) or `viewBuilder` (the no-code spec)",
                    )
                })?;
            normalized.insert("view_query".to_string(), Value::String(query.to_string()));
            normalized
                .entry("view_origin".to_string())
                .or_insert_with(|| Value::String("sql".to_string()));
        }
    }

    Ok(Value::Object(normalized))
}

/// Create a view collection: a read-only collection over a Postgres `VIEW`.
async fn create_view(client: &mut Client, input: CreateCollection) -> Result<Collection> {
    let name = input.name.clone();
    if find(client, &name).await?.is_some() {
        return Err(Error::conflict(format!(
            "a collection named {name:?} already exists"
        )));
    }
    if table_exists(client, &name).await? {
        return Err(Error::conflict(format!(
            "a table or view named {name:?} already exists"
        )));
    }
    if !input.fields.is_empty() {
        return Err(Error::invalid(
            "a view's fields are its query's columns; send `viewQuery` or `viewBuilder` instead of fields",
        ));
    }

    let all = list(client).await?;
    let options = view_options(&input.options, &all)?;
    let query = options
        .get("view_query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let id = lazypock_core::id::new_id();
    let mut collection = Collection {
        id: id.clone(),
        name: name.clone(),
        collection_type: CollectionType::View,
        options,
        rules: input.rules,
        fields: Vec::new(),
        created: SystemTime::now(),
        updated: SystemTime::now(),
    };

    let transaction = client.transaction().await.map_err(db_error)?;
    transaction
        .batch_execute(&ddl::create_view_statement(&name, &query)?)
        .await
        .map_err(|err| Error::invalid(format!("failed to create the view {name:?}: {err}")))?;

    let row = transaction
        .query_one(
            "insert into _collections (id, name, type, options, rules) \
             values ($1, $2, $3, $4, $5) returning created, updated",
            &[
                &collection.id,
                &collection.name,
                &collection.collection_type.as_str(),
                &collection.options,
                &collection.rules,
            ],
        )
        .await
        .map_err(db_error)?;
    collection.created = row.get("created");
    collection.updated = row.get("updated");

    collection.fields = view_fields(&transaction, &name, &id).await?;
    if !collection.fields.iter().any(|field| field.name == "id") {
        return Err(Error::invalid(format!(
            "the view {name:?} must expose an `id` column; the records API addresses records by id"
        )));
    }

    // No indexes (a view cannot have them), no policies (Postgres has no RLS on a
    // view; `security_invoker` makes the source tables' policies apply instead) and
    // no realtime trigger (a view fires none).
    notify_schema_changed(&transaction).await?;
    transaction.commit().await.map_err(db_error)?;

    tracing::info!(collection = %collection.name, "view collection created");
    Ok(collection)
}

/// Update a view collection: its rules, its definition, or both.
async fn update_view(
    client: &mut Client,
    existing: Collection,
    input: UpdateCollection,
) -> Result<Collection> {
    let all = list(client).await?;
    let options = match input.options {
        Some(options) => view_options(&options, &all)?,
        None => existing.options.clone(),
    };
    let query = options
        .get("view_query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query_changed = query
        != existing
            .options
            .get("view_query")
            .and_then(Value::as_str)
            .unwrap_or_default();
    let rules = input.rules.unwrap_or(existing.rules.clone());

    // The Studio loads a view's derived fields and sends them back unchanged; that is
    // not an attempt to edit the columns, so it is accepted. A payload that names
    // different fields is, and it is refused: the columns come from the query.
    if let Some(fields) = &input.fields {
        let mut current: Vec<&str> = existing
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect();
        let mut declared: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
        current.sort_unstable();
        declared.sort_unstable();
        if !declared.is_empty() && current != declared {
            return Err(Error::invalid(
                "a view's columns come from its query; change `viewQuery` (or `viewBuilder`) instead of its fields",
            ));
        }
    }

    let transaction = client.transaction().await.map_err(db_error)?;
    if query_changed {
        transaction
            .batch_execute(&ddl::replace_view_statement(&existing.name, &query)?)
            .await
            .map_err(|err| {
                Error::invalid(format!(
                    "failed to replace the view {:?}: {err}. A view can only be replaced with a \
                     compatible column list; drop and re-create it to rename or remove a column",
                    existing.name
                ))
            })?;
    }

    transaction
        .execute(
            "update _collections set rules = $2, options = $3, updated = now() where id = $1",
            &[&existing.id, &rules, &options],
        )
        .await
        .map_err(db_error)?;

    let mut updated = existing;
    updated.rules = rules;
    updated.options = options;
    updated.fields = view_fields(&transaction, &updated.name, &updated.id).await?;

    notify_schema_changed(&transaction).await?;
    transaction.commit().await.map_err(db_error)?;

    get(client, &updated.id).await
}

/// Find a collection by id or name.
pub async fn find(client: &Client, name_or_id: &str) -> Result<Option<Collection>> {
    Ok(list(client)
        .await?
        .into_iter()
        .find(|collection| collection.id == name_or_id || collection.name == name_or_id))
}

/// Get a collection by id or name, failing when it does not exist.
pub async fn get(client: &Client, name_or_id: &str) -> Result<Collection> {
    find(client, name_or_id)
        .await?
        .ok_or_else(|| Error::not_found(format!("no collection named {name_or_id:?}")))
}

/// Create a collection: table, indexes and metadata, atomically.
///
/// A **view** collection is created as a Postgres `VIEW` with `security_invoker = true`
/// (see [`create_view`]): it is read-only, its fields are the query's columns, and it
/// has no indexes, policies or realtime trigger.
pub async fn create(client: &mut Client, input: CreateCollection) -> Result<Collection> {
    // Validated as given, never silently normalised: `create("Posts")` is an
    // error rather than a collection that is quietly named "posts".
    let name = input.name.clone();
    validate_collection_name(&name)?;

    if input.collection_type == CollectionType::View {
        return create_view(client, input).await;
    }

    if find(client, &name).await?.is_some() {
        return Err(Error::conflict(format!(
            "a collection named {name:?} already exists"
        )));
    }

    let id = lazypock_core::id::new_id();
    let fields = input
        .fields
        .iter()
        .enumerate()
        .map(|(position, field)| {
            Ok(Field {
                id: lazypock_core::id::new_id(),
                collection_id: id.clone(),
                name: field.name.clone(),
                field_type: field.field_type,
                required: field.required,
                is_unique: field.unique,
                options: field.options.clone(),
                position: position as i32,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut collection = Collection {
        id: id.clone(),
        name: name.clone(),
        collection_type: input.collection_type,
        options: input.options,
        rules: input.rules,
        fields,
        created: SystemTime::now(),
        updated: SystemTime::now(),
    };
    collection.validate()?;

    // A table may exist without a `_collections` row: created by hand, or left
    // behind by `delete(.., drop_data = false)`. Attaching to it is opt-in so an
    // unrelated table is never silently claimed.
    let existing_table = table_exists(client, &name).await?;
    if existing_table && !input.adopt_existing_table {
        return Err(Error::conflict(format!(
            "a table named {name:?} already exists; set adoptExistingTable to attach to it"
        )));
    }
    if existing_table {
        verify_adoptable(client, &collection).await?;
    }

    // Resolve relation targets, including a self-reference.
    let mut targets = load_table_names(client).await?;
    targets.insert(id.clone(), name.clone());

    // A relation may **name** its target rather than identify it. PocketBase stores
    // the target's id, but the implementation this replaced — and the Studio built
    // for it — send `options.collection` with a name, and here a collection's name
    // *is* its table name, so a name is a valid key in its own right. Without this the
    // resolver was id-only and a name was reported as an unknown collection, which is
    // what stopped a relation field being created at all.
    for table in targets.values().cloned().collect::<Vec<_>>() {
        targets.entry(table.clone()).or_insert(table);
    }

    let resolve = |target: &str| targets.get(target).cloned();
    // Read once: the rule compiler needs the other collections to whitelist
    // dotted paths, and this must happen before the transaction borrows `client`.
    let all = list(client).await?;
    let create_table = if existing_table {
        None
    } else {
        Some(ddl::create_table_statement(&collection, &resolve)?)
    };

    let transaction = client.transaction().await.map_err(db_error)?;

    if let Some(statement) = create_table {
        transaction.batch_execute(&statement).await.map_err(|err| {
            Error::invalid(format!("failed to create the table for {name:?}: {err}"))
        })?;
    }

    // Read the timestamps back: the database owns them, and returning the stored
    // value keeps what the caller receives identical to what is persisted.
    let row = transaction
        .query_one(
            "insert into _collections (id, name, type, options, rules) \
             values ($1, $2, $3, $4, $5) returning created, updated",
            &[
                &collection.id,
                &collection.name,
                &collection.collection_type.as_str(),
                &collection.options,
                &collection.rules,
            ],
        )
        .await
        .map_err(db_error)?;
    collection.created = row.get("created");
    collection.updated = row.get("updated");

    for field in &collection.fields {
        insert_field(&transaction, field).await?;
        if field.is_unique && !existing_table {
            let statement = ddl::create_unique_index_statement(&collection.name, &field.name);
            transaction.batch_execute(&statement).await.map_err(|err| {
                Error::invalid(format!(
                    "failed to create the unique index for {name:?}.{:?}: {err}",
                    field.name
                ))
            })?;
        } else if field.is_indexed() && !existing_table {
            // The field-level `indexed` flag: a plain index, which the documented field
            // options promise and nothing created.
            let statement = ddl::create_index_statement(&collection.name, &field.name);
            transaction.batch_execute(&statement).await.map_err(|err| {
                Error::invalid(format!(
                    "failed to create the index for {name:?}.{:?}: {err}",
                    field.name
                ))
            })?;
        }
    }

    // An auth collection has fields the framework owns, whether or not the caller
    // declared them. Recording them here — and not only in the migration that
    // backfilled the collections which already existed — is what stops a collection
    // created afterwards from showing no email and no password, which is the bug this
    // fixes. `on conflict do nothing` so a caller who declared one keeps their version.
    if collection.collection_type.is_auth() {
        for (index, (field_name, field_type, required, unique, options)) in
            types::auth_system_fields().into_iter().enumerate()
        {
            transaction
                .execute(
                    "insert into _fields \
                     (id, collection_id, name, type, required, is_unique, options, position) \
                     values ($1, $2, $3, $4, $5, $6, $7, $8) \
                     on conflict (collection_id, name) do nothing",
                    &[
                        &lazypock_core::id::new_id(),
                        &collection.id,
                        &field_name.to_string(),
                        &field_type.as_str(),
                        &required,
                        &unique,
                        &options,
                        &((index + 1) as i32),
                    ],
                )
                .await
                .map_err(db_error)?;
        }
    }

    // An auth collection needs case-insensitive unique emails: login looks the
    // account up by `lower(email)`, so a duplicate would be ambiguous.
    if collection.collection_type.is_auth() && !existing_table {
        transaction
            .batch_execute(&ddl::create_auth_email_index_statement(&collection.name))
            .await
            .map_err(|err| {
                Error::invalid(format!("failed to make emails unique in {name:?}: {err}"))
            })?;
    }

    // Row-level security is applied as part of creating a collection rather than
    // left to the caller, so a new call site cannot forget it and end up with a
    // table that has no policies at all.
    let rule_context = FilterContext::for_collection(&collection, &all)?;

    // Custom indexes last, because a partial index's predicate is compiled against
    // the same context the rules use. `None` for the previous declaration: this is a
    // new collection, so every declared index is new.
    apply_declared_indexes(&transaction, &collection, &rule_context, None).await?;

    rls::policies::apply(&transaction, &collection, &rule_context).await?;

    // The change-event trigger, for the same reason policies are applied here: a
    // collection that is not wired up would silently never publish anything.
    if !existing_table {
        transaction
            .batch_execute(&ddl::create_realtime_trigger_statement(&collection.name)?)
            .await
            .map_err(|err| {
                Error::invalid(format!(
                    "failed to add the realtime trigger to {name:?}: {err}"
                ))
            })?;
    }

    notify_schema_changed(&transaction).await?;
    transaction.commit().await.map_err(db_error)?;

    tracing::info!(collection = %collection.name, "collection created");
    Ok(collection)
}

/// Refuse to adopt an existing table that lacks the columns the collection
/// declares, so stored metadata can never misdescribe the real schema.
async fn verify_adoptable(client: &Client, collection: &Collection) -> Result<()> {
    let rows = client
        .query(
            "select column_name from information_schema.columns \
             where table_schema = current_schema() and table_name = $1",
            &[&collection.name],
        )
        .await
        .map_err(db_error)?;
    let present: HashSet<String> = rows.iter().map(|row| row.get::<_, String>(0)).collect();

    let mut expected: Vec<&str> = types::SYSTEM_COLUMNS.to_vec();
    if collection.collection_type == CollectionType::Auth {
        expected.extend_from_slice(types::AUTH_SYSTEM_COLUMNS);
    }
    for field in &collection.fields {
        expected.push(field.name.as_str());
    }

    let missing: Vec<&str> = expected
        .into_iter()
        .filter(|column| !present.contains(*column))
        .collect();
    if !missing.is_empty() {
        return Err(Error::invalid(format!(
            "cannot adopt the existing table {:?}: it is missing the column(s) {}",
            collection.name,
            missing.join(", ")
        )));
    }
    Ok(())
}

/// The `indexes` strings a collection's options declare.
fn declared_indexes(options: &Value) -> Result<Vec<String>> {
    let Some(value) = options.get("indexes").filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    let Some(entries) = value.as_array() else {
        return Err(Error::invalid(format!(
            "`options.indexes` must be a list of index declarations, got {value}"
        )));
    };
    entries
        .iter()
        .map(|entry| {
            entry.as_str().map(str::to_string).ok_or_else(|| {
                Error::invalid(format!(
                    "an index declaration must be a string, got {entry}"
                ))
            })
        })
        .collect()
}

/// Apply a collection's `options.indexes` declaration.
///
/// Runs inside the caller's transaction, so a bad index leaves neither the table nor
/// the metadata changed. `previous_options` is the declaration as it was before this
/// call (`None` for a collection being created), which is what lets a removed or
/// changed index be dropped rather than accumulating.
async fn apply_declared_indexes(
    transaction: &tokio_postgres::Transaction<'_>,
    collection: &Collection,
    context: &FilterContext,
    previous_options: Option<&Value>,
) -> Result<()> {
    let declared = declared_indexes(&collection.options)?;
    let specs = indexes::parse_all(collection, &declared, context)?;

    let existing: HashSet<String> = transaction
        .query(
            "select indexname from pg_indexes \
             where schemaname = current_schema() and tablename = $1",
            &[&collection.name],
        )
        .await
        .map_err(db_error)?
        .iter()
        .map(|row| row.get::<_, String>(0))
        .collect();

    // Best effort: a previous declaration that no longer parses named a column that
    // has since been dropped, and Postgres dropped the index along with the column.
    let previous = match previous_options {
        None => Vec::new(),
        Some(options) => {
            indexes::parse_all(collection, &declared_indexes(options)?, context).unwrap_or_default()
        }
    };

    for old in &previous {
        // Rebuilt indexes are dropped first; an unchanged one is left in place, so
        // saving a collection does not rebuild every index it has.
        if specs.iter().any(|spec| spec == old) {
            continue;
        }
        if !existing.contains(&old.name) {
            continue;
        }
        transaction
            .batch_execute(&old.drop_statement())
            .await
            .map_err(|err| {
                Error::invalid(format!(
                    "failed to drop the index {:?} on {:?}: {err}",
                    old.name, collection.name
                ))
            })?;
    }

    for spec in &specs {
        let unchanged = previous.iter().any(|old| old == spec) && existing.contains(&spec.name);
        if unchanged {
            continue;
        }
        if existing.contains(&spec.name) && !previous.iter().any(|old| old.name == spec.name) {
            // An index with that name exists but was not created from this
            // declaration (reflected from a hand-made table, or created outside
            // lazypock). Replacing it could drop a constraint lazypock depends on —
            // the case-insensitive unique email index is the one that matters — so it
            // is left alone and said out loud instead.
            tracing::warn!(
                collection = %collection.name,
                index = %spec.name,
                "an index with this name already exists and was left as it is; the declared \
                 index was not applied. Rename the declared index to apply it."
            );
            continue;
        }
        transaction
            .batch_execute(&spec.create_statement(&collection.name))
            .await
            .map_err(|err| {
                Error::invalid(format!(
                    "failed to create the index {:?} on {:?}: {err}",
                    spec.name, collection.name
                ))
            })?;
    }

    Ok(())
}

/// Delete a collection's metadata, and optionally its data.
///
/// Keeping the data (`drop_data = false`) is useful for recovering from an
/// accidental metadata delete, and is what an undo flow would use.
/// Update one existing field's metadata.
///
/// The **type is not changed**: altering a column's type is a migration, not an
/// edit, and doing it silently would either fail or lose data. A changed type is
/// refused by name so the caller learns why instead of finding the field unchanged.
/// Everything else — required, unique, options — is applied, including the unique
/// index that `unique` implies.
async fn update_field(
    client: &mut Client,
    table: &str,
    current: &Field,
    wanted: &CreateField,
) -> Result<()> {
    if current.field_type != wanted.field_type {
        return Err(Error::invalid(format!(
            "the field {:?} is a {} and cannot become a {} in place; remove it and add \
             it again if the type must change",
            wanted.name,
            current.field_type.as_str(),
            wanted.field_type.as_str()
        )));
    }

    let transaction = client.transaction().await.map_err(db_error)?;

    // The index a field implies is real DDL, so it has to follow the flags. `unique`
    // and `indexed` share one index name (`idx_<table>_<column>`), so the transition
    // is always "drop what is there, then create what is wanted": dropping first is
    // what lets a unique index become a plain one (or the reverse) at all.
    let wanted_index = if wanted.unique {
        Some(ddl::create_unique_index_statement(table, &wanted.name))
    } else if crate::types::option_bool(&wanted.options, "indexed") {
        Some(ddl::create_index_statement(table, &wanted.name))
    } else {
        None
    };
    let current_index = if current.is_unique {
        Some(ddl::create_unique_index_statement(table, &current.name))
    } else if current.is_indexed() {
        Some(ddl::create_index_statement(table, &current.name))
    } else {
        None
    };
    if current_index != wanted_index {
        if current_index.is_some() {
            transaction
                .batch_execute(&ddl::drop_index_statement(table, &wanted.name))
                .await
                .map_err(db_error)?;
        }
        if let Some(statement) = wanted_index {
            transaction.batch_execute(&statement).await.map_err(|err| {
                Error::invalid(format!(
                    "failed to update the index for {:?}.{:?}: {err}",
                    table, wanted.name
                ))
            })?;
        }
    }

    // The default is part of the column's DDL, so changing it is a real change to the
    // table rather than only to the metadata.
    if current.default_value()
        != wanted
            .options
            .get("default")
            .filter(|value| !value.is_null())
    {
        let column = ddl::quote_ident(&wanted.name);
        let statement = match crate::types::default_sql(wanted.field_type, &wanted.options)? {
            Some(literal) => format!(
                "alter table {} alter column {column} set default {literal}",
                ddl::quote_ident(table)
            ),
            None => format!(
                "alter table {} alter column {column} drop default",
                ddl::quote_ident(table)
            ),
        };
        transaction.batch_execute(&statement).await.map_err(|err| {
            Error::invalid(format!(
                "failed to change the default of {:?}.{:?}: {err}",
                table, wanted.name
            ))
        })?;
    }

    transaction
        .execute(
            "update _fields set required = $2, is_unique = $3, options = $4, \
             updated = now() where id = $1",
            &[
                &current.id,
                &wanted.required,
                &wanted.unique,
                &wanted.options,
            ],
        )
        .await
        .map_err(db_error)?;

    notify_schema_changed(&transaction).await?;
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

/// Input for updating a collection's metadata.
///
/// Rules and options are replaced as given, and `fields` is a declaration the
/// collection's columns are reconciled against: named fields are added or updated,
/// and undeclared ones are only dropped when the caller says so.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCollection {
    /// Replacement access rules, when given.
    pub rules: Option<Value>,
    /// Replacement type-specific settings, when given.
    pub options: Option<Value>,
    /// Fields to add or update. Fields **absent** from this list are left alone by
    /// default, so a payload that omits one does not drop its column; removing a
    /// field stays an explicit `delete_field`.
    #[serde(default)]
    pub fields: Option<Vec<CreateField>>,
    /// Drop the fields this collection has and `fields` did not declare.
    ///
    /// This is the destructive half of an import: a collection definition that
    /// names some columns and *omits* others is a statement about the whole column
    /// set, and the caller that sent it asked for the omission to be applied.
    /// Off by default, and not reachable from a request body (`serde(skip)`), so a
    /// client cannot ask for a column drop through the update endpoint — the Studio
    /// removes a field with an explicit `delete_field`. Built-in columns are never
    /// dropped, whatever this says.
    #[serde(skip)]
    pub delete_missing_fields: bool,
}

/// Update a collection's rules and options.
///
/// Absent keys are left as they were, so a caller can change a rule without
/// sending the rest of the definition back.
pub async fn update(
    client: &mut Client,
    name_or_id: &str,
    input: UpdateCollection,
) -> Result<Collection> {
    let existing = get(client, name_or_id).await?;

    if existing.collection_type == CollectionType::View {
        return update_view(client, existing, input).await;
    }

    let rules = input.rules.unwrap_or(existing.rules.clone());
    let options = input.options.unwrap_or(existing.options.clone());

    // Fields first, and outside the transaction below: adding one runs DDL, and
    // `add_field` manages its own transaction.
    //
    // Ignoring them was the first version of this, and it was the worst possible
    // behaviour: a collection editor sent the whole definition, got a 200, and its
    // field changes — a relation field most visibly — silently did nothing.
    if let Some(fields) = &input.fields {
        for field in fields {
            match existing
                .fields
                .iter()
                .find(|current| current.name == field.name)
            {
                None => {
                    add_field(client, &existing.name, field.clone()).await?;
                }
                Some(current) => {
                    update_field(client, &existing.name, current, field).await?;
                }
            }
        }
    }

    // After the incoming fields are in place, so a drop can never see a column the
    // payload was about to (re)declare as missing.
    if input.delete_missing_fields {
        if let Some(fields) = &input.fields {
            let declared: HashSet<&str> = fields.iter().map(|field| field.name.as_str()).collect();
            drop_undeclared_fields(client, &existing, &declared).await?;
        }
    }

    // Re-read after the field changes above: the policy context below has to see
    // the fields that now exist, not the ones the collection started with.
    let mut updated = get(client, &existing.id).await?;
    updated.rules = rules;
    updated.options = options;

    // The rule compiler resolves relation paths, so it needs the other
    // collections. Read once, before the transaction borrows `client`.
    let all = list(client).await?;

    // In a transaction with the notification, so subscribers are told only once
    // the change is actually visible — the same pattern the other mutations use.
    let transaction = client.transaction().await.map_err(db_error)?;
    transaction
        .execute(
            "update _collections set rules = $2, options = $3, updated = now() where id = $1",
            &[&existing.id, &updated.rules, &updated.options],
        )
        .await
        .map_err(db_error)?;

    // Persist the order the client sent. The incoming array is the order the
    // Studio's field list is in after a drag, and the read side now exposes the
    // stored position as `sort_order`; without this a reorder was accepted and
    // silently dropped, so the editor's drag never survived a refresh.
    if let Some(fields) = &input.fields {
        for (position, field) in fields.iter().enumerate() {
            transaction
                .execute(
                    "update _fields set position = $3 where collection_id = $1 and name = $2",
                    &[&existing.id, &field.name, &(position as i32)],
                )
                .await
                .map_err(db_error)?;
        }
    }

    // Rebuild the policies from the new rules. Without this the row changed but
    // the table kept the policies it already had, so a rule edited to public in
    // the Studio was stored and displayed but never enforced, and an
    // unauthenticated read returned an empty list. `apply` rebuilds the policies
    // wholesale, so a rule that was removed stops applying too.
    let rule_context = FilterContext::for_collection(&updated, &all)?;

    // The index list is a declaration as well: declared indexes are created or
    // rebuilt, and one the new declaration dropped goes away. `existing.options` is
    // the declaration before this call, which is what says what to drop.
    apply_declared_indexes(
        &transaction,
        &updated,
        &rule_context,
        Some(&existing.options),
    )
    .await?;

    rls::policies::apply(&transaction, &updated, &rule_context).await?;

    notify_schema_changed(&transaction).await?;
    transaction.commit().await.map_err(db_error)?;

    get(client, &existing.id).await
}

pub async fn delete(client: &mut Client, name_or_id: &str, drop_data: bool) -> Result<Collection> {
    let collection = get(client, name_or_id).await?;

    let transaction = client.transaction().await.map_err(db_error)?;

    if drop_data && collection.collection_type == CollectionType::View {
        transaction
            .batch_execute(&ddl::drop_view_statement(&collection.name)?)
            .await
            .map_err(|err| {
                Error::internal(format!(
                    "failed to drop the view for {:?}: {err}",
                    collection.name
                ))
            })?;
    } else if drop_data && collection.collection_type.is_table() {
        transaction
            .batch_execute(&ddl::drop_table_statement(&collection.name)?)
            .await
            .map_err(|err| {
                Error::internal(format!(
                    "failed to drop the table for {:?}: {err}",
                    collection.name
                ))
            })?;
    }

    transaction
        .execute("delete from _collections where id = $1", &[&collection.id])
        .await
        .map_err(db_error)?;

    notify_schema_changed(&transaction).await?;
    transaction.commit().await.map_err(db_error)?;

    tracing::info!(collection = %collection.name, drop_data, "collection deleted");
    Ok(collection)
}

/// Add a field to an existing collection, backfilling safely.
pub async fn add_field(client: &mut Client, name_or_id: &str, input: CreateField) -> Result<Field> {
    let collection = get(client, name_or_id).await?;
    if !collection.collection_type.is_table() {
        return Err(Error::unsupported(format!(
            "cannot add a field to the {:?} collection {:?}",
            collection.collection_type, collection.name
        )));
    }
    if collection.field(&input.name).is_some() {
        return Err(Error::conflict(format!(
            "collection {:?} already has a field named {:?}",
            collection.name, input.name
        )));
    }

    let field = Field {
        id: lazypock_core::id::new_id(),
        collection_id: collection.id.clone(),
        name: input.name,
        field_type: input.field_type,
        required: input.required,
        is_unique: input.unique,
        options: input.options,
        position: collection
            .fields
            .iter()
            .map(|existing| existing.position)
            .max()
            .map_or(0, |max| max + 1),
    };
    field.validate(collection.collection_type)?;

    let statements = ddl::add_column_statements(&collection.name, &field)?;

    let transaction = client.transaction().await.map_err(db_error)?;
    for statement in statements {
        transaction.batch_execute(&statement).await.map_err(|err| {
            Error::invalid(format!(
                "failed to add field {:?} to {:?}: {err}",
                field.name, collection.name
            ))
        })?;
    }
    if field.is_unique {
        let statement = ddl::create_unique_index_statement(&collection.name, &field.name);
        transaction.batch_execute(&statement).await.map_err(|err| {
            Error::invalid(format!(
                "failed to add a unique index for {:?}.{:?}: {err}",
                collection.name, field.name
            ))
        })?;
    } else if field.is_indexed() {
        let statement = ddl::create_index_statement(&collection.name, &field.name);
        transaction.batch_execute(&statement).await.map_err(|err| {
            Error::invalid(format!(
                "failed to add an index for {:?}.{:?}: {err}",
                collection.name, field.name
            ))
        })?;
    }
    insert_field(&transaction, &field).await?;
    notify_schema_changed(&transaction).await?;
    transaction.commit().await.map_err(db_error)?;

    tracing::info!(collection = %collection.name, field = %field.name, "field added");
    Ok(field)
}

/// Remove a field and its column.
pub async fn delete_field(
    client: &mut Client,
    name_or_id: &str,
    field_name: &str,
) -> Result<Field> {
    let collection = get(client, name_or_id).await?;
    let field = collection.field(field_name).cloned().ok_or_else(|| {
        Error::not_found(format!(
            "collection {:?} has no field named {field_name:?}",
            collection.name
        ))
    })?;

    let transaction = client.transaction().await.map_err(db_error)?;

    transaction
        .batch_execute(&ddl::drop_column_statement(&collection.name, &field.name)?)
        .await
        .map_err(|err| {
            Error::internal(format!(
                "failed to drop {:?}.{:?}: {err}",
                collection.name, field.name
            ))
        })?;

    transaction
        .execute("delete from _fields where id = $1", &[&field.id])
        .await
        .map_err(db_error)?;

    notify_schema_changed(&transaction).await?;
    transaction.commit().await.map_err(db_error)?;

    tracing::info!(collection = %collection.name, field = %field.name, "field deleted");
    Ok(field)
}

/// Whether a column is lazypock's own and must never be dropped.
///
/// `email` is only built-in on an auth collection: a base collection is free to have
/// a column of that name, and dropping it on request is not the same act.
pub fn is_builtin_column(collection_type: CollectionType, name: &str) -> bool {
    types::SYSTEM_COLUMNS.contains(&name)
        || (collection_type == CollectionType::Auth && types::AUTH_SYSTEM_COLUMNS.contains(&name))
}

/// The fields a declarative update would drop: the collection has them, `declared`
/// does not name them, and they are not lazypock's own columns.
///
/// Public because an import has to be able to say what it is about to drop *before*
/// it drops it — the report it returns names them — and the rule for what counts is
/// not something a second caller should be free to restate.
pub fn undeclared_fields<'a>(
    collection: &'a Collection,
    declared: &HashSet<&str>,
) -> Vec<&'a Field> {
    collection
        .fields
        .iter()
        .filter(|field| !declared.contains(field.name.as_str()))
        .filter(|field| !is_builtin_column(collection.collection_type, &field.name))
        .collect()
}

/// Drop every column `existing` has that `declared` does not name.
///
/// One transaction for the whole set, and one notification at the end: a caller
/// applying a declaration drops a column *because* it is absent from that
/// declaration, so the set is a single schema change and subscribers should reload
/// once. Built-in columns are skipped rather than dropped, which is what keeps an
/// auth collection's `email`/`verified`/`password_hash` (rows in `_fields`, but
/// lazypock's own columns) out of reach of an import.
///
/// Returns the names dropped.
async fn drop_undeclared_fields(
    client: &mut Client,
    existing: &Collection,
    declared: &HashSet<&str>,
) -> Result<Vec<String>> {
    let doomed = undeclared_fields(existing, declared);

    if doomed.is_empty() {
        return Ok(Vec::new());
    }

    let transaction = client.transaction().await.map_err(db_error)?;
    for field in &doomed {
        transaction
            .batch_execute(&ddl::drop_column_statement(&existing.name, &field.name)?)
            .await
            .map_err(|err| {
                Error::internal(format!(
                    "failed to drop {:?}.{:?}: {err}",
                    existing.name, field.name
                ))
            })?;
        transaction
            .execute("delete from _fields where id = $1", &[&field.id])
            .await
            .map_err(db_error)?;
    }
    notify_schema_changed(&transaction).await?;
    transaction.commit().await.map_err(db_error)?;

    let dropped: Vec<String> = doomed.iter().map(|field| field.name.clone()).collect();
    tracing::info!(
        collection = %existing.name,
        fields = ?dropped,
        "undeclared fields dropped"
    );
    Ok(dropped)
}

async fn insert_field(transaction: &tokio_postgres::Transaction<'_>, field: &Field) -> Result<()> {
    transaction
        .execute(
            "insert into _fields \
             (id, collection_id, name, type, required, is_unique, options, position) \
             values ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &field.id,
                &field.collection_id,
                &field.name,
                &field.field_type.as_str(),
                &field.required,
                &field.is_unique,
                &field.options,
                &field.position,
            ],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Emit the schema-changed notification.
///
/// Called inside the mutation's transaction: `NOTIFY` is delivered at commit, so
/// other nodes only ever see an applied change.
async fn notify_schema_changed(transaction: &tokio_postgres::Transaction<'_>) -> Result<()> {
    transaction
        .execute("select pg_notify($1, $2)", &[&SCHEMA_CHANGED_CHANNEL, &""])
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Map of collection id to table name.
pub async fn load_table_names(client: &Client) -> Result<HashMap<String, String>> {
    let rows = client
        .query("select id, name from _collections", &[])
        .await
        .map_err(db_error)?;
    Ok(rows
        .iter()
        .map(|row| (row.get::<_, String>("id"), row.get::<_, String>("name")))
        .collect())
}

/// Whether a relation (table or view) with this name already exists.
pub async fn table_exists(client: &Client, name: &str) -> Result<bool> {
    let row = client
        .query_one(
            "select to_regclass($1) is not null",
            &[&ddl::quote_ident(name)],
        )
        .await
        .map_err(db_error)?;
    Ok(row.get(0))
}

#[cfg(test)]
mod field_shape_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_field_accepts_both_shapes_for_its_settings() {
        // PocketBase, and therefore Studio, puts a field type's settings at the
        // field's top level; this server's own format nests them under `options`.
        // Serde drops unknown keys, so before this the flat shape arrived with its
        // settings silently discarded and validation then reported the option
        // missing — "a relation field needs a `collectionId` option" — which named
        // the symptom and hid the cause.
        let flat: CreateField = serde_json::from_value(json!({
            "name": "author",
            "type": "relation",
            "required": false,
            "unique": false,
            "collectionId": "abc123",
            "maxSelect": 1
        }))
        .expect("the PocketBase shape");
        assert_eq!(flat.options["collectionId"], json!("abc123"));
        assert_eq!(flat.options["maxSelect"], json!(1));

        let nested: CreateField = serde_json::from_value(json!({
            "name": "author",
            "type": "relation",
            "options": { "collectionId": "abc123", "maxSelect": 1 }
        }))
        .expect("this server's shape");
        assert_eq!(nested.options["collectionId"], json!("abc123"));

        // Both at once: the explicit container wins, so a caller can override.
        let both: CreateField = serde_json::from_value(json!({
            "name": "author",
            "type": "relation",
            "collectionId": "loses",
            "options": { "collectionId": "wins" }
        }))
        .expect("both");
        assert_eq!(both.options["collectionId"], json!("wins"));

        // The field's own attributes are not mistaken for settings.
        assert_eq!(flat.name, "author");
        assert!(!flat.required);
        assert!(!flat.unique);
        assert_eq!(flat.field_type, FieldType::Relation);
    }

    #[test]
    fn a_select_field_takes_its_values_from_either_shape() {
        // The same mechanism, for another field type: it was never specific to
        // relations, which is why the fix belongs in the parsing rather than in the
        // relation branch.
        let flat: CreateField = serde_json::from_value(json!({
            "name": "state",
            "type": "select",
            "values": ["draft", "live"]
        }))
        .expect("the PocketBase shape");
        assert_eq!(flat.options["values"], json!(["draft", "live"]));
    }
}
