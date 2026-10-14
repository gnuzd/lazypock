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
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateField {
    /// Field name.
    pub name: String,
    /// Field type.
    #[serde(rename = "type")]
    pub field_type: FieldType,
    /// Whether the column is required.
    #[serde(default)]
    pub required: bool,
    /// Whether the column is unique.
    #[serde(default)]
    pub unique: bool,
    /// Type-specific settings.
    #[serde(default = "empty_object")]
    pub options: Value,
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
