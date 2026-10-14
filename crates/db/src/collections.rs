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
use serde_json::Value;
use tokio_postgres::Client;

use crate::ddl;
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
}

/// A field as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// 15-character record id.
    pub id: String,
    /// Owning collection id.
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

    Ok(collections)
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
pub async fn create(client: &mut Client, input: CreateCollection) -> Result<Collection> {
    // Validated as given, never silently normalised: `create("Posts")` is an
    // error rather than a collection that is quietly named "posts".
    let name = input.name.clone();
    validate_collection_name(&name)?;

    if input.collection_type == CollectionType::View {
        return Err(Error::unsupported(
            "view collections are not implemented yet (PLAN.md section 4.4)",
        ));
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
    let resolve = |collection_id: &str| targets.get(collection_id).cloned();
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

    // A unique index is real DDL, so it has to follow the flag.
    if wanted.unique && !current.is_unique {
        transaction
            .batch_execute(&crate::ddl::create_unique_index_statement(
                table,
                &wanted.name,
            ))
            .await
            .map_err(db_error)?;
    } else if !wanted.unique && current.is_unique {
        transaction
            .batch_execute(&crate::ddl::drop_index_statement(table, &wanted.name))
            .await
            .map_err(db_error)?;
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
/// Only rules and options: changing a table's shape is a field operation, which
/// goes through `add_field`/`delete_field` so the DDL stays in one place.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCollection {
    /// Replacement access rules, when given.
    pub rules: Option<Value>,
    /// Replacement type-specific settings, when given.
    pub options: Option<Value>,
    /// Fields to add or update. Fields **absent** from this list are left alone, so
    /// a payload that omits one does not drop its column; removing a field stays an
    /// explicit `delete_field`.
    #[serde(default)]
    pub fields: Option<Vec<CreateField>>,
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

    let rules = input.rules.unwrap_or(existing.rules.clone());
    let options = input.options.unwrap_or(existing.options.clone());

    // Fields first, and outside the transaction below: adding one runs DDL, and
    // `add_field` manages its own transaction.
    //
    // Ignoring them was the first version of this, and it was the worst possible
    // behaviour: a collection editor sent the whole definition, got a 200, and its
    // field changes — a relation field most visibly — silently did nothing.
    if let Some(fields) = input.fields {
        for field in fields {
            match existing
                .fields
                .iter()
                .find(|current| current.name == field.name)
            {
                None => {
                    add_field(client, &existing.name, field).await?;
                }
                Some(current) => {
                    update_field(client, &existing.name, current, &field).await?;
                }
            }
        }
    }

    // In a transaction with the notification, so subscribers are told only once
    // the change is actually visible — the same pattern the other mutations use.
    let transaction = client.transaction().await.map_err(db_error)?;
    transaction
        .execute(
            "update _collections set rules = $2, options = $3, updated = now() where id = $1",
            &[&existing.id, &rules, &options],
        )
        .await
        .map_err(db_error)?;
    notify_schema_changed(&transaction).await?;
    transaction.commit().await.map_err(db_error)?;

    get(client, &existing.id).await
}

pub async fn delete(client: &mut Client, name_or_id: &str, drop_data: bool) -> Result<Collection> {
    let collection = get(client, name_or_id).await?;

    let transaction = client.transaction().await.map_err(db_error)?;

    if drop_data && collection.collection_type.is_table() {
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
