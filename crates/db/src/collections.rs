//! Create / change / delete collections.
//!
//! Every mutation is **one transaction**: it locks the collection's metadata row,
//! runs the DDL, updates `_collections` / `_fields`, re-applies the RLS policies
//! and emits `NOTIFY lazypock_schema_changed`. Postgres DDL is transactional, so
//! a failure leaves neither half behind.
//!
//! These functions do **not** refresh the in-memory schema cache. Other nodes
//! learn through the NOTIFY (see `watch`); on this node use [`crate::store::SchemaStore`],
//! which calls these and then reloads, so a request right after a change sees it.

use crate::ddl;
use crate::error::{self, PgExt};
use crate::ident::Ident;
use crate::params::typed;
use crate::pool::Db;
use crate::rls;
use crate::schema::{self, AUTH_COLUMNS, Collection, Field, Rules, auth_system_fields};
use crate::types::{CollectionType, FieldType};
use lazypock_core::id::new_id;
use tokio_postgres::Transaction;

use lazypock_core::Result;

pub const SCHEMA_CHANGED_CHANNEL: &str = "lazypock_schema_changed";

fn json_err(e: serde_json::Error) -> lazypock_core::Error {
    error::invalid_schema(e.to_string())
}

/// What the caller may change: system columns of an auth collection are managed here.
fn is_system(c: &Collection, name: &str) -> bool {
    schema::SYSTEM_FIELDS.contains(&name) || (c.ty.is_auth() && AUTH_COLUMNS.contains(&name))
}

/// A copy without the auth system fields, so `validate()` (which reserves their
/// names) only sees what the caller declared.
fn declared_only(c: &Collection) -> Collection {
    let mut copy = c.clone();
    if c.ty.is_auth() {
        copy.fields
            .retain(|f| !AUTH_COLUMNS.contains(&f.name.as_str()));
    }
    copy
}

pub async fn create(db: &Db, input: &Collection) -> Result<Collection> {
    input.validate()?;

    let mut stored = input.clone();
    stored.id = new_id();
    stored.fields = Vec::new();
    let mut position = 0;
    if input.ty == CollectionType::Auth {
        stored.fields.extend(auth_system_fields());
        position = 5;
    }
    for f in &input.fields {
        let mut f = f.clone();
        f.id = new_id();
        f.position = position;
        position += 1;
        stored.fields.push(f);
    }

    let mut conn = db.conn().await?;
    let tx = rls::begin_owner(&mut conn).await?;

    tx.batch_execute(&ddl::create_table(input)?.join(";\n"))
        .await
        .pg()?;

    let options = stored.options.clone();
    let rules = serde_json::to_value(&stored.rules).map_err(json_err)?;
    let ty = stored.ty.as_str();
    tx.query_typed(
        "insert into _collections (id, name, type, options, rules) values ($1, $2, $3, $4, $5)",
        &[
            typed!(stored.id, TEXT),
            typed!(stored.name.as_str(), TEXT),
            typed!(ty, TEXT),
            typed!(options, JSONB),
            typed!(rules, JSONB),
        ],
    )
    .await
    .pg()?;
    for f in &stored.fields {
        insert_field(&tx, &stored.id, f).await?;
    }

    rls::apply(&tx, &stored).await?;
    finish(tx).await?;
    Ok(stored)
}

pub async fn delete(db: &Db, name: &Ident) -> Result<()> {
    let mut conn = db.conn().await?;
    let tx = rls::begin_owner(&mut conn).await?;
    let c = load(&tx, name, true).await?;
    ensure_editable(&c)?;
    tx.batch_execute(&ddl::drop_table(name)).await.pg()?;
    tx.query_typed(
        "delete from _fields where collection_id = $1",
        &[typed!(c.id, TEXT)],
    )
    .await
    .pg()?;
    tx.query_typed(
        "delete from _collections where id = $1",
        &[typed!(c.id, TEXT)],
    )
    .await
    .pg()?;
    finish(tx).await
}

pub async fn rename(db: &Db, from: &Ident, to: &Ident) -> Result<()> {
    let mut conn = db.conn().await?;
    let tx = rls::begin_owner(&mut conn).await?;
    let c = load(&tx, from, true).await?;
    ensure_editable(&c)?;
    tx.batch_execute(&ddl::rename_table(from, to)).await.pg()?;
    tx.query_typed(
        "update _collections set name = $1, updated = now() where id = $2",
        &[typed!(to.as_str(), TEXT), typed!(c.id, TEXT)],
    )
    .await
    .pg()?;
    finish(tx).await
}

pub async fn add_field(db: &Db, collection: &Ident, field: &Field) -> Result<Field> {
    let mut conn = db.conn().await?;
    let tx = rls::begin_owner(&mut conn).await?;
    let c = load(&tx, collection, true).await?;
    ensure_editable(&c)?;

    let mut with = declared_only(&c);
    with.fields.push(field.clone());
    with.validate()?; // system names, duplicates, supported type, rules still compile
    if is_system(&c, field.name.as_str()) || c.fields.iter().any(|f| f.name == field.name) {
        return Err(error::invalid_schema(format!(
            "field {:?} already exists",
            field.name.as_str()
        )));
    }

    let mut f = field.clone();
    f.id = new_id();
    tx.batch_execute(&ddl::add_column(collection, &f)?.join(";\n"))
        .await
        .pg()?;
    let next = tx
        .query_typed(
            "select coalesce(max(position), -1) + 1 from _fields where collection_id = $1",
            &[typed!(c.id, TEXT)],
        )
        .await
        .pg()?[0]
        .get::<_, i32>(0);
    f.position = next;
    insert_field(&tx, &c.id, &f).await?;
    touch(&tx, &c.id).await?;
    finish(tx).await?;
    Ok(f)
}

pub async fn drop_field(db: &Db, collection: &Ident, field: &Ident) -> Result<()> {
    let mut conn = db.conn().await?;
    let tx = rls::begin_owner(&mut conn).await?;
    let c = load(&tx, collection, true).await?;
    ensure_editable(&c)?;
    if is_system(&c, field.as_str()) {
        return Err(error::invalid_schema(format!(
            "{:?} is a system field and cannot be removed",
            field.as_str()
        )));
    }
    if !c.fields.iter().any(|f| f.name == *field) {
        return Err(error::not_found(format!("field {field}")));
    }
    // Postgres refuses (-> Conflict) while a policy still depends on the column.
    tx.batch_execute(&ddl::drop_column(collection, field))
        .await
        .pg()?;
    tx.query_typed(
        "delete from _fields where collection_id = $1 and name = $2",
        &[typed!(c.id, TEXT), typed!(field.as_str(), TEXT)],
    )
    .await
    .pg()?;
    touch(&tx, &c.id).await?;
    finish(tx).await
}

/// Renaming a field that a stored rule mentions is refused: the rule text would
/// go stale. Change the rule first, then rename.
pub async fn rename_field(db: &Db, collection: &Ident, from: &Ident, to: &Ident) -> Result<()> {
    let mut conn = db.conn().await?;
    let tx = rls::begin_owner(&mut conn).await?;
    let mut c = load(&tx, collection, true).await?;
    ensure_editable(&c)?;
    if is_system(&c, from.as_str()) {
        return Err(error::invalid_schema(format!(
            "{:?} is a system field and cannot be renamed",
            from.as_str()
        )));
    }
    let Some(f) = c.fields.iter_mut().find(|f| f.name == *from) else {
        return Err(error::not_found(format!("field {from}")));
    };
    let was_unique = f.is_unique;
    f.name = to.clone();
    declared_only(&c).validate()?; // new name is free, and the rules still compile

    tx.batch_execute(&ddl::rename_column(collection, from, to))
        .await
        .pg()?;
    if was_unique {
        tx.batch_execute(&ddl::rename_index(collection, from, to))
            .await
            .pg()?;
    }
    tx.query_typed(
        "update _fields set name = $1, updated = now() where collection_id = $2 and name = $3",
        &[
            typed!(to.as_str(), TEXT),
            typed!(c.id, TEXT),
            typed!(from.as_str(), TEXT),
        ],
    )
    .await
    .pg()?;
    touch(&tx, &c.id).await?;
    finish(tx).await
}

pub async fn set_rules(db: &Db, collection: &Ident, rules: &Rules) -> Result<()> {
    let mut conn = db.conn().await?;
    let tx = rls::begin_owner(&mut conn).await?;
    let mut c = load(&tx, collection, true).await?;
    ensure_editable(&c)?;
    c.rules = rules.clone();
    declared_only(&c).validate()?;
    let json = serde_json::to_value(rules).map_err(json_err)?;
    tx.query_typed(
        "update _collections set rules = $1, updated = now() where id = $2",
        &[typed!(json, JSONB), typed!(c.id, TEXT)],
    )
    .await
    .pg()?;
    rls::apply(&tx, &c).await?;
    finish(tx).await
}

pub async fn get(db: &Db, name: &Ident) -> Result<Collection> {
    let mut conn = db.conn().await?;
    let tx = rls::begin_owner(&mut conn).await?;
    let c = load(&tx, name, false).await?;
    tx.commit().await.pg()?;
    Ok(c)
}

pub async fn list(db: &Db) -> Result<Vec<Collection>> {
    let conn = db.conn().await?;
    schema::load_all(&**conn).await
}

// ---- internals -------------------------------------------------------------

fn ensure_editable(c: &Collection) -> Result<()> {
    if c.ty == CollectionType::View {
        return Err(error::invalid_schema(
            "view collections are not supported yet",
        ));
    }
    Ok(())
}

/// Load one collection; with `lock`, hold its metadata row until commit so two
/// schema edits to the same collection cannot interleave.
async fn load(tx: &Transaction<'_>, name: &Ident, lock: bool) -> Result<Collection> {
    let sql = if lock {
        "select id, name, type, options, rules from _collections where name = $1 for update"
    } else {
        "select id, name, type, options, rules from _collections where name = $1"
    };
    let rows = tx
        .query_typed(sql, &[typed!(name.as_str(), TEXT)])
        .await
        .pg()?;
    let row = rows
        .first()
        .ok_or_else(|| error::not_found(format!("collection {name}")))?;
    let id: String = row.get(0);
    let rules: serde_json::Value = row.get(4);
    let mut c = Collection {
        id: id.clone(),
        name: name.clone(),
        ty: CollectionType::parse(&row.get::<_, String>(2))?,
        options: row.get(3),
        rules: serde_json::from_value(rules).map_err(json_err)?,
        fields: Vec::new(),
    };
    let frows = tx
        .query_typed(
            "select id, name, type, required, is_unique, options, position from _fields \
             where collection_id = $1 order by position, name",
            &[typed!(id, TEXT)],
        )
        .await
        .pg()?;
    for r in frows {
        c.fields.push(Field {
            id: r.get(0),
            name: Ident::new(r.get::<_, String>(1))?,
            ty: FieldType::parse(&r.get::<_, String>(2))?,
            required: r.get::<_, Option<bool>>(3).unwrap_or(false),
            is_unique: r.get::<_, Option<bool>>(4).unwrap_or(false),
            options: r
                .get::<_, Option<serde_json::Value>>(5)
                .unwrap_or_else(|| serde_json::json!({})),
            position: r.get::<_, Option<i32>>(6).unwrap_or(0),
        });
    }
    Ok(c)
}

async fn insert_field(tx: &Transaction<'_>, collection_id: &str, f: &Field) -> Result<()> {
    let ty = f.ty.as_str();
    tx.query_typed(
        "insert into _fields (id, collection_id, name, type, required, is_unique, options, position) \
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
        &[
            typed!(f.id, TEXT),
            typed!(collection_id, TEXT),
            typed!(f.name.as_str(), TEXT),
            typed!(ty, TEXT),
            typed!(f.required, BOOL),
            typed!(f.is_unique, BOOL),
            typed!(f.options, JSONB),
            typed!(f.position, INT4),
        ],
    )
    .await
    .pg()?;
    Ok(())
}

async fn touch(tx: &Transaction<'_>, collection_id: &str) -> Result<()> {
    tx.query_typed(
        "update _collections set updated = now() where id = $1",
        &[typed!(collection_id, TEXT)],
    )
    .await
    .pg()?;
    Ok(())
}

/// NOTIFY is transactional: listeners are told only if the change commits.
async fn finish(tx: Transaction<'_>) -> Result<()> {
    tx.batch_execute(&format!("select pg_notify('{SCHEMA_CHANGED_CHANNEL}', '')"))
        .await
        .pg()?;
    tx.commit().await.pg()
}
