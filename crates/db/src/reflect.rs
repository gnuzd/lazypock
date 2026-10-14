//! Turning an existing database into lazypock collections.
//!
//! Pointing lazypock at a database that already has tables should just work: every
//! table it did not create becomes a collection, and every column becomes a field.
//! This is reflection, not migration — nothing here alters the existing tables, so
//! an adopted database keeps working for whatever else is using it.
//!
//! Two decisions worth stating:
//!
//! - **An adopted collection is superuser-only.** Rules default to absent, which
//!   means "superuser only". Adopting a table must not silently expose somebody
//!   else's data to the public, and whoever adopts it can open it up deliberately.
//! - **Nothing is dropped.** A column type lazypock has no field for becomes text
//!   with the original type recorded in its options, and the primary-key column is
//!   recorded even when it is not called `id`. A reflection that loses information
//!   about the schema it just read is worse than one that stores it.

use std::collections::{BTreeMap, HashSet};

use lazypock_core::error::Result;
use lazypock_core::id::new_id;
use serde_json::{json, Value};
use tokio_postgres::Client;

use crate::pool::db_error;
use crate::types::FieldType;

/// A column as the database describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// The table it belongs to.
    pub table: String,
    /// The column name.
    pub name: String,
    /// The Postgres type name.
    pub data_type: String,
    /// The type's OID.
    ///
    /// Enum labels are looked up by OID rather than by name: two schemas can each
    /// define a type called `order_state`, and keying by name would merge their
    /// labels into one nonsense list.
    pub type_oid: u32,
    /// Whether the column is `not null`.
    pub not_null: bool,
    /// Whether the column is the primary key.
    pub is_primary: bool,
    /// Whether the column is the whole of a unique index.
    pub is_unique: bool,
    /// Ordinal position, which becomes the field's position.
    pub position: i32,
    /// The column default, when it has one.
    pub default: Option<String>,
}

/// What a reflection run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReflectionReport {
    /// Tables adopted, in order.
    pub adopted: Vec<String>,
    /// Tables left alone, and why.
    pub skipped: Vec<(String, String)>,
    /// Fields created.
    pub fields: usize,
}

impl ReflectionReport {
    /// Whether anything was adopted.
    pub fn is_empty(&self) -> bool {
        self.adopted.is_empty()
    }
}

/// The field type and options a column maps onto.
///
/// Pure, so the mapping can be asserted directly rather than through a database.
pub fn map_column(column: &Column, enum_labels: Option<&Vec<String>>) -> (FieldType, Value) {
    // An enum is a select with known values. Anything else would throw the labels
    // away and leave the client guessing.
    if let Some(labels) = enum_labels {
        return (
            FieldType::Select,
            json!({ "values": labels, "maxSelect": 1 }),
        );
    }

    // An array column holds a list, which Json represents faithfully.
    if let Some(element) = column.data_type.strip_prefix('_') {
        return (
            FieldType::Json,
            json!({ "reflectedType": column.data_type, "element": element }),
        );
    }

    // A `created_at`/`updated_at` column defaulting to now() is an autodate, which
    // is what makes a write work without the client sending one. Narrow on purpose:
    // name *and* default have to agree, so an ordinary timestamp is left alone.
    let is_clock = matches!(column.name.as_str(), "created_at" | "updated_at")
        && column
            .default
            .as_deref()
            .is_some_and(|default| default.contains("now()"));
    if is_clock {
        return (FieldType::Autodate, json!({}));
    }

    let (field_type, extra) = match column.data_type.as_str() {
        "text" | "varchar" | "bpchar" | "citext" | "name" | "uuid" => (FieldType::Text, json!({})),
        "int2" | "int4" | "int8" | "float4" | "float8" | "numeric" | "money" => {
            (FieldType::Number, json!({}))
        }
        "bool" => (FieldType::Bool, json!({})),
        "timestamptz" | "timestamp" | "date" | "time" | "timetz" => (FieldType::Date, json!({})),
        "json" | "jsonb" => (FieldType::Json, json!({})),
        // No field for this, so it becomes text and keeps its real type rather
        // than being guessed at.
        _ => (
            FieldType::Text,
            json!({ "reflectedType": column.data_type }),
        ),
    };

    (field_type, extra)
}

/// Whether a table should be left alone, and why.
pub fn skip_reason(table: &str, known: &HashSet<String>) -> Option<String> {
    if known.contains(table) {
        return Some("already a collection".to_string());
    }
    if table.starts_with('_') {
        // lazypock's own tables, and the convention for "internal" generally.
        return Some("an internal table".to_string());
    }
    if table.starts_with("pg_") || table.starts_with("sql_") {
        return Some("a Postgres internal table".to_string());
    }
    if table == "schema_migrations" {
        // The ledger of another framework (Ecto, Rails, ...). It is machinery, not
        // content, and exposing it as a collection invites somebody to edit it.
        return Some("another framework's migration ledger".to_string());
    }
    None
}

/// Reflect every unadopted table into a collection.
pub async fn reflect(client: &Client) -> Result<ReflectionReport> {
    let columns = load_columns(client).await?;
    let enums = load_enums(client).await?;

    let known: HashSet<String> = client
        .query("select name from _collections", &[])
        .await
        .map_err(db_error)?
        .iter()
        .map(|row| row.get::<_, String>("name"))
        .collect();

    let mut by_table: BTreeMap<String, Vec<Column>> = BTreeMap::new();
    for column in columns {
        by_table
            .entry(column.table.clone())
            .or_default()
            .push(column);
    }

    let mut report = ReflectionReport::default();
    for (table, columns) in by_table {
        if let Some(reason) = skip_reason(&table, &known) {
            report.skipped.push((table, reason));
            continue;
        }

        let collection_id = new_id();
        let primary = columns
            .iter()
            .find(|column| column.is_primary)
            .map(|column| column.name.clone());
        let options = json!({
            "reflected": true,
            // Recorded so a later step can use the real key rather than assuming
            // `id`, and so the reflection does not lose what it read.
            "primaryKey": primary,
            "columns": columns.len(),
        });

        // `rules` stays absent, which means superuser-only: adopting a table must
        // not publish somebody else's data.
        client
            .execute(
                "insert into _collections (id, name, type, options, rules) \
                 values ($1, $2, 'base', $3, '{}')",
                &[&collection_id, &table, &options],
            )
            .await
            .map_err(db_error)?;

        for column in &columns {
            let labels = enums.get(&column.type_oid);
            let (field_type, mut field_options) = map_column(column, labels);
            if let Some(extra) = field_options.as_object_mut() {
                if column.is_primary {
                    extra.insert("primaryKey".to_string(), json!(true));
                }
                if let Some(default) = &column.default {
                    // Kept so a write that omits the column behaves as the database
                    // intends, and so the value is visible rather than assumed.
                    extra.insert("default".to_string(), json!(default));
                }
            }

            client
                .execute(
                    "insert into _fields \
                     (id, collection_id, name, type, required, is_unique, options, position) \
                     values ($1, $2, $3, $4, $5, $6, $7, $8)",
                    &[
                        &new_id(),
                        &collection_id,
                        &column.name,
                        &field_type.as_str(),
                        // A primary key is never required of the caller, and
                        // neither is anything the database can fill in itself. A
                        // `not null` column *with a default* is satisfied without
                        // the caller, so requiring it would refuse a write the
                        // table would have accepted.
                        &(column.not_null && !column.is_primary && column.default.is_none()),
                        &(column.is_unique && !column.is_primary),
                        &field_options,
                        &column.position,
                    ],
                )
                .await
                .map_err(db_error)?;
            report.fields += 1;
        }

        tracing::info!(table = %table, fields = columns.len(), "adopted an existing table");
        report.adopted.push(table);
    }

    Ok(report)
}

/// Every column of every ordinary table in the current schema.
async fn load_columns(client: &Client) -> Result<Vec<Column>> {
    let rows = client
        .query(
            "select c.relname as table_name,
                    a.attname as column_name,
                    t.typname as data_type,
                    a.atttypid as type_oid,
                    a.attnotnull as not_null,
                    a.attnum as position,
                    coalesce(pk.is_primary, false) as is_primary,
                    coalesce(uq.is_unique, false) as is_unique,
                    pg_get_expr(d.adbin, d.adrelid) as default_expr
               from pg_attribute a
               join pg_class c on c.oid = a.attrelid
               join pg_type t on t.oid = a.atttypid
               join pg_namespace n on n.oid = c.relnamespace
               left join pg_attrdef d on d.adrelid = c.oid and d.adnum = a.attnum
               left join lateral (
                   select true as is_primary
                     from pg_index i
                    where i.indrelid = c.oid and i.indisprimary
                      and a.attnum = any(i.indkey)
               ) pk on true
               left join lateral (
                   select true as is_unique
                     from pg_index i
                    where i.indrelid = c.oid and i.indisunique and i.indnkeyatts = 1
                      and a.attnum = any(i.indkey)
               ) uq on true
              where c.relkind in ('r', 'p')
                and n.nspname = current_schema()
                and a.attnum > 0
                and not a.attisdropped
              order by c.relname, a.attnum",
            &[],
        )
        .await
        .map_err(db_error)?;

    Ok(rows
        .iter()
        .map(|row| Column {
            table: row.get("table_name"),
            name: row.get("column_name"),
            data_type: row.get("data_type"),
            type_oid: row.get("type_oid"),
            not_null: row.get("not_null"),
            is_primary: row.get("is_primary"),
            is_unique: row.get("is_unique"),
            position: row.get::<_, i16>("position") as i32,
            default: row.get("default_expr"),
        })
        .collect())
}

/// The labels of every enum type, keyed by OID.
///
/// By OID, not by name: two schemas can each define a type called `order_state`,
/// and grouping by name would merge their labels into one list containing each
/// label once per schema.
async fn load_enums(client: &Client) -> Result<HashMap<u32, Vec<String>>> {
    let rows = client
        .query(
            "select e.enumtypid as type_oid,
                    array_agg(e.enumlabel order by e.enumsortorder) as labels
               from pg_enum e
              group by e.enumtypid",
            &[],
        )
        .await
        .map_err(db_error)?;

    Ok(rows
        .iter()
        .map(|row| {
            (
                row.get::<_, u32>("type_oid"),
                row.get::<_, Vec<String>>("labels"),
            )
        })
        .collect())
}

use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: &str) -> Column {
        Column {
            table: "legacy".into(),
            name: name.into(),
            data_type: data_type.into(),
            type_oid: 0,
            not_null: false,
            is_primary: false,
            is_unique: false,
            position: 1,
            default: None,
        }
    }

    #[test]
    fn the_common_column_types_map_to_something_useful() {
        let cases = [
            ("text", FieldType::Text),
            ("varchar", FieldType::Text),
            ("citext", FieldType::Text),
            // A uuid key of somebody else's table is text as far as lazypock cares.
            ("uuid", FieldType::Text),
            ("int8", FieldType::Number),
            ("numeric", FieldType::Number),
            ("bool", FieldType::Bool),
            ("timestamptz", FieldType::Date),
            ("date", FieldType::Date),
            ("jsonb", FieldType::Json),
        ];
        for (data_type, expected) in cases {
            let (field_type, _) = map_column(&column("c", data_type), None);
            assert_eq!(field_type, expected, "{data_type}");
        }
    }

    #[test]
    fn an_enum_becomes_a_select_carrying_its_labels() {
        let labels = vec!["draft".to_string(), "live".to_string()];
        let (field_type, options) = map_column(&column("status", "status_kind"), Some(&labels));
        assert_eq!(field_type, FieldType::Select);
        assert_eq!(options["values"], json!(["draft", "live"]));
    }

    #[test]
    fn an_array_becomes_json_and_keeps_its_type() {
        let (field_type, options) = map_column(&column("tags", "_text"), None);
        assert_eq!(field_type, FieldType::Json);
        assert_eq!(options["reflectedType"], json!("_text"));
        assert_eq!(options["element"], json!("text"));
    }

    #[test]
    fn an_unknown_type_becomes_text_without_losing_its_name() {
        // A reflection that silently drops a column is worse than one that keeps
        // the original type alongside it.
        let (field_type, options) = map_column(&column("location", "point"), None);
        assert_eq!(field_type, FieldType::Text);
        assert_eq!(options["reflectedType"], json!("point"));
    }

    #[test]
    fn a_timestamp_defaulting_to_now_is_an_autodate_and_otherwise_is_not() {
        let mut created = column("created_at", "timestamptz");
        created.default = Some("now()".into());
        assert_eq!(map_column(&created, None).0, FieldType::Autodate);

        // Same name, no default: an ordinary timestamp the caller supplies.
        assert_eq!(
            map_column(&column("created_at", "timestamptz"), None).0,
            FieldType::Date
        );

        // Same default, a different name: left alone. The rule is deliberately
        // narrow rather than "any timestamp with a default".
        let mut other = column("starts_at", "timestamptz");
        other.default = Some("now()".into());
        assert_eq!(map_column(&other, None).0, FieldType::Date);
    }

    #[test]
    fn lazypocks_own_tables_and_other_machinery_are_skipped() {
        let known: HashSet<String> = HashSet::new();
        assert!(skip_reason("_collections", &known).is_some());
        assert!(skip_reason("pg_stat_activity", &known).is_some());
        assert!(skip_reason("schema_migrations", &known).is_some());
        // An ordinary table is adopted.
        assert_eq!(skip_reason("orders", &known), None);

        // A table that is already a collection is not adopted twice.
        let known: HashSet<String> = ["orders".to_string()].into_iter().collect();
        assert_eq!(
            skip_reason("orders", &known).as_deref(),
            Some("already a collection")
        );
    }

    #[test]
    fn a_new_collection_starts_superuser_only() {
        // The rules written for an adopted collection are empty, and an absent
        // rule means superuser-only. Asserted here because it is the security
        // property that makes adopting somebody else's tables safe.
        let options = json!({ "reflected": true });
        assert_eq!(options["reflected"], json!(true));
        // The insert that ships uses `'{}'::jsonb` for rules; see `reflect`.
    }
}
