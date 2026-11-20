//! The model of a collection (as stored in `_collections` / `_fields`) and the
//! in-memory snapshot of all of them.
//!
//! Readers use [`SchemaCache`]: a lock-free `Arc` swap, so a request never waits
//! for a reload and never touches the database to learn a collection's shape.

use crate::error::{self, PgExt};
use crate::ident::Ident;
use crate::types::{CollectionType, FieldType, Kind};
use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio_postgres::GenericClient;

use lazypock_core::Result;

pub const MAX_FIELDS: usize = 100;
/// Columns every collection table has.
pub const SYSTEM_FIELDS: [&str; 3] = ["id", "created", "updated"];
/// Columns an auth collection has on top (see migration 0006 for the field twin).
pub const AUTH_COLUMNS: [&str; 5] = [
    "email",
    "password_hash",
    "verified",
    "token_key",
    "emailVisibility",
];
/// Credentials that must never be returned to a client.
pub const CREDENTIAL_COLUMNS: [&str; 2] = ["password_hash", "token_key"];

fn empty_object() -> Value {
    json!({})
}

fn base() -> CollectionType {
    CollectionType::Base
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    #[serde(default)]
    pub id: String,
    pub name: Ident,
    #[serde(rename = "type")]
    pub ty: FieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default, rename = "unique")]
    pub is_unique: bool,
    #[serde(default = "empty_object")]
    pub options: Value,
    #[serde(default)]
    pub position: i32,
}

impl Field {
    pub fn new(name: &str, ty: FieldType) -> Result<Self> {
        Ok(Field {
            id: String::new(),
            name: Ident::new(name)?,
            ty,
            required: false,
            is_unique: false,
            options: empty_object(),
            position: 0,
        })
    }

    /// Hidden fields never appear in responses or in filters/sorts.
    pub fn hidden(&self) -> bool {
        self.ty == FieldType::Password
            || self
                .options
                .get("hidden")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    }

    pub fn kind(&self) -> Option<Kind> {
        self.ty.kind(&self.options)
    }
}

/// Access rules. `None` = locked, `Some("")` = public, `Some(expr)` = filter.
/// Keys are camelCase in storage (`listRule`, ...); unknown keys are preserved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rules {
    #[serde(default)]
    pub list_rule: Option<String>,
    #[serde(default)]
    pub view_rule: Option<String>,
    #[serde(default)]
    pub create_rule: Option<String>,
    #[serde(default)]
    pub update_rule: Option<String>,
    #[serde(default)]
    pub delete_rule: Option<String>,
    #[serde(default)]
    pub manage_rule: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    #[serde(default)]
    pub id: String,
    pub name: Ident,
    #[serde(rename = "type", default = "base")]
    pub ty: CollectionType,
    #[serde(default = "empty_object")]
    pub options: Value,
    #[serde(default)]
    pub rules: Rules,
    #[serde(default)]
    pub fields: Vec<Field>,
}

impl Collection {
    pub fn new(name: &str, ty: CollectionType) -> Result<Self> {
        Ok(Collection {
            id: String::new(),
            name: Ident::new(name)?,
            ty,
            options: empty_object(),
            rules: Rules::default(),
            fields: Vec::new(),
        })
    }

    /// Structural checks, then a dry-run compile of every rule so a bad rule is
    /// rejected when it is saved, not when the first request hits it.
    pub fn validate(&self) -> Result<()> {
        if self.ty == CollectionType::View {
            return Err(error::invalid_schema(
                "view collections are not supported yet",
            ));
        }
        if self.fields.len() > MAX_FIELDS {
            return Err(error::invalid_schema(format!(
                "at most {MAX_FIELDS} fields"
            )));
        }
        let mut seen = HashSet::new();
        for f in &self.fields {
            let name = f.name.as_str();
            let reserved = SYSTEM_FIELDS.contains(&name)
                || (self.ty.is_auth() && AUTH_COLUMNS.contains(&name));
            if reserved {
                return Err(error::invalid_schema(format!("{name:?} is a system field")));
            }
            if !seen.insert(name) {
                return Err(error::invalid_schema(format!("duplicate field {name:?}")));
            }
            f.ty.column_type(&f.options)?;
            if f.ty == FieldType::Password {
                return Err(error::invalid_schema(
                    "password fields are reserved for auth collections",
                ));
            }
            if f.options.get("default").is_some_and(|d| !d.is_null()) {
                return Err(error::invalid_schema(
                    "the `default` option is not supported yet",
                ));
            }
        }
        crate::rls::validate_rules(self)
    }

    /// How a filter/sort may use a column; `None` for anything unknown or hidden.
    pub fn column_kind(&self, name: &str) -> Option<Kind> {
        match name {
            "id" => Some(Kind::Text),
            "created" | "updated" => Some(Kind::Date),
            _ => self
                .fields
                .iter()
                .find(|f| f.name.as_str() == name && !f.hidden())
                .and_then(Field::kind),
        }
    }

    /// Columns a response may include, in order. Credentials are never among
    /// them: `password_hash` is a hidden field and `token_key` is not a field.
    pub fn visible_columns(&self) -> Vec<&str> {
        let mut cols = vec!["id", "created", "updated"];
        cols.extend(
            self.fields
                .iter()
                .filter(|f| !f.hidden() && !CREDENTIAL_COLUMNS.contains(&f.name.as_str()))
                .map(|f| f.name.as_str()),
        );
        cols
    }
}

/// The fields an auth collection has whether or not anybody declared them
/// (the Rust twin of migration 0006).
pub(crate) fn auth_system_fields() -> Vec<Field> {
    let f = |name: &str, ty, required, is_unique, options: Value, position| Field {
        id: lazypock_core::id::new_id(),
        name: Ident::new(name).expect("static name"),
        ty,
        required,
        is_unique,
        options,
        position,
    };
    vec![
        f("email", FieldType::Email, true, true, json!({}), 1),
        f(
            "password_hash",
            FieldType::Password,
            false,
            false,
            json!({"hidden": true, "writeOnly": true}),
            2,
        ),
        f(
            "verified",
            FieldType::Bool,
            false,
            false,
            json!({"hidden": true}),
            3,
        ),
        f(
            "emailVisibility",
            FieldType::Bool,
            false,
            false,
            json!({}),
            4,
        ),
    ]
}

// ---- loading ---------------------------------------------------------------

/// Read every collection with its fields. A row that cannot be understood (a
/// name that fails validation, an unknown type) is skipped with a warning, so
/// one bad row never takes the whole schema down.
pub async fn load_all<C: GenericClient>(client: &C) -> Result<Vec<Collection>> {
    let rows = client
        .query_typed(
            "select c.id, c.name, c.type, c.options, c.rules, \
                    f.id, f.name, f.type, f.required, f.is_unique, f.options, f.position \
             from _collections c left join _fields f on f.collection_id = c.id \
             order by c.name, f.position, f.name",
            &[],
        )
        .await
        .pg()?;

    let mut out: Vec<Collection> = Vec::new();
    let mut current: Option<Collection> = None;
    let mut skip_id = String::new();

    for row in rows {
        let cid: String = row.get(0);
        if cid == skip_id {
            continue;
        }
        if current.as_ref().map(|c| c.id.as_str()) != Some(cid.as_str()) {
            out.extend(current.take());
            match collection_header(&row) {
                Ok(c) => current = Some(c),
                Err(e) => {
                    tracing::warn!(collection = %row.get::<_, String>(1), error = %e, "skipping a collection that cannot be loaded");
                    skip_id = cid;
                    continue;
                }
            }
        }
        if row.get::<_, Option<String>>(5).is_none() {
            continue;
        }
        match field_from_row(&row) {
            Ok(f) => current.as_mut().expect("set above").fields.push(f),
            Err(e) => {
                tracing::warn!(collection = %row.get::<_, String>(1), error = %e, "skipping a field that cannot be loaded")
            }
        }
    }
    out.extend(current);
    Ok(out)
}

fn collection_header(row: &tokio_postgres::Row) -> Result<Collection> {
    let rules: Value = row.get(4);
    Ok(Collection {
        id: row.get(0),
        name: Ident::new(row.get::<_, String>(1))?,
        ty: CollectionType::parse(&row.get::<_, String>(2))?,
        options: row.get(3),
        rules: serde_json::from_value(rules)
            .map_err(|e| error::invalid_schema(format!("rules: {e}")))?,
        fields: Vec::new(),
    })
}

fn field_from_row(row: &tokio_postgres::Row) -> Result<Field> {
    Ok(Field {
        id: row.get(5),
        name: Ident::new(row.get::<_, String>(6))?,
        ty: FieldType::parse(&row.get::<_, String>(7))?,
        required: row.get::<_, Option<bool>>(8).unwrap_or(false),
        is_unique: row.get::<_, Option<bool>>(9).unwrap_or(false),
        options: row.get::<_, Option<Value>>(10).unwrap_or_else(empty_object),
        position: row.get::<_, Option<i32>>(11).unwrap_or(0),
    })
}

// ---- snapshot --------------------------------------------------------------

#[derive(Debug, Default)]
pub struct Schema {
    by_name: HashMap<String, Arc<Collection>>,
}

impl Schema {
    pub fn get(&self, name: &str) -> Option<&Arc<Collection>> {
        self.by_name.get(name)
    }
    pub fn get_by_id(&self, id: &str) -> Option<&Arc<Collection>> {
        self.by_name.values().find(|c| c.id == id)
    }
    pub fn len(&self) -> usize {
        self.by_name.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Arc<Collection>> {
        self.by_name.values()
    }
}

#[derive(Debug)]
pub struct SchemaCache(ArcSwap<Schema>);

impl Default for SchemaCache {
    fn default() -> Self {
        SchemaCache(ArcSwap::from_pointee(Schema::default()))
    }
}

impl SchemaCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The current snapshot. Cheap; hold it for the duration of one request.
    pub fn snapshot(&self) -> Arc<Schema> {
        self.0.load_full()
    }

    pub fn get(&self, name: &str) -> Option<Arc<Collection>> {
        self.0.load().get(name).cloned()
    }

    pub fn len(&self) -> usize {
        self.0.load().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Rebuild the snapshot from the database. On error the previous snapshot is
    /// kept (the caller decides whether that is fatal).
    pub async fn reload<C: GenericClient>(&self, client: &C) -> Result<Arc<Schema>> {
        let by_name = load_all(client)
            .await?
            .into_iter()
            .map(|c| (c.name.as_str().to_owned(), Arc::new(c)))
            .collect();
        let schema = Arc::new(Schema { by_name });
        self.0.store(schema.clone());
        Ok(schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coll(fields: Vec<Field>) -> Collection {
        let mut c = Collection::new("posts", CollectionType::Base).unwrap();
        c.fields = fields;
        c
    }

    #[test]
    fn rejects_system_duplicate_and_unsupported_fields() {
        assert!(
            coll(vec![Field::new("id", FieldType::Text).unwrap()])
                .validate()
                .is_err()
        );
        assert!(
            coll(vec![
                Field::new("a", FieldType::Text).unwrap(),
                Field::new("a", FieldType::Bool).unwrap()
            ])
            .validate()
            .is_err()
        );
        assert!(
            coll(vec![Field::new("r", FieldType::Relation).unwrap()])
                .validate()
                .is_err()
        );
        assert!(
            coll(vec![Field::new("p", FieldType::Password).unwrap()])
                .validate()
                .is_err()
        );
        assert!(
            coll(vec![Field::new("title", FieldType::Text).unwrap()])
                .validate()
                .is_ok()
        );
        let mut auth = Collection::new("users", CollectionType::Auth).unwrap();
        auth.fields = vec![Field::new("email", FieldType::Text).unwrap()];
        assert!(
            auth.validate().is_err(),
            "auth system column names are reserved"
        );
        let mut v = Collection::new("v", CollectionType::View).unwrap();
        v.fields = vec![];
        assert!(v.validate().is_err());
    }

    #[test]
    fn hidden_fields_are_neither_visible_nor_filterable() {
        let mut c = Collection::new("users", CollectionType::Auth).unwrap();
        c.fields = auth_system_fields();
        assert_eq!(c.column_kind("email"), Some(Kind::Text));
        assert_eq!(c.column_kind("password_hash"), None);
        assert_eq!(c.column_kind("token_key"), None);
        assert_eq!(c.column_kind("verified"), None, "hidden");
        let cols = c.visible_columns();
        assert!(cols.contains(&"email") && cols.contains(&"emailVisibility"));
        assert!(
            !cols.contains(&"password_hash")
                && !cols.contains(&"token_key")
                && !cols.contains(&"verified")
        );
    }

    #[test]
    fn rules_round_trip_and_keep_unknown_keys() {
        let stored =
            json!({"listRule": "", "viewRule": null, "createRule": "a = 1", "authRule": "x"});
        let rules: Rules = serde_json::from_value(stored.clone()).unwrap();
        assert_eq!(rules.list_rule.as_deref(), Some(""));
        assert_eq!(rules.view_rule, None);
        assert_eq!(serde_json::to_value(&rules).unwrap()["authRule"], "x");
        assert_eq!(serde_json::to_value(&rules).unwrap()["createRule"], "a = 1");
    }
}
