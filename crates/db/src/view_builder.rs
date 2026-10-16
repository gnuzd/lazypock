//! Generates the SQL for a "view builder" collection from a no-code spec.
//!
//! A port of the reference implementation (`Lazypock.Schema.ViewBuilder` in the
//! Elixir tree), kept faithful deliberately: the generated statement is not an
//! implementation detail, it is what the DDL engine introspects and what the view
//! is created from. A different but equivalent statement would be a different
//! view.
//!
//! The spec lives in a collection's `options["view_builder"]` and the generated
//! SQL in `options["view_query"]`, so a hand-written view and a built one are the
//! same thing to everything downstream.
//!
//! ## Spec
//!
//! ```json
//! {
//!   "source": "orders",
//!   "relations": [{ "alias": "t1", "field": "customer" }],
//!   "fields": [
//!     { "source": "orders", "name": "id" },
//!     { "source": "t1", "name": "name", "as": "customer_name" }
//!   ],
//!   "sort": "-created_at",
//!   "limit": 100
//! }
//! ```
//!
//! * `source` — required base collection.
//! * `relations` — a relation field whose target's columns are pulled in. Only
//!   the alias and the field are given; the target and its multiplicity come from
//!   the field's own metadata.
//! * `fields` — `source` is the base collection or a declared alias; `as` is the
//!   output name, defaulting to the column name for base columns and
//!   `<relation_field>_<name>` for related ones. The `id` column is injected when
//!   absent, and output names are de-duplicated with a numeric suffix so a
//!   collision cannot produce invalid SQL.
//! * `sort` — PocketBase-style, over **base** columns: comma separated, a `-`
//!   prefix descending.
//! * `limit` — optional positive integer.
//!
//! ## Joins, and why they are not all joins
//!
//! A **single** relation becomes a `LEFT JOIN … ON alias."id"::text =
//! source.field`. At most one target row matches, so the base `id` stays unique —
//! which is a hard requirement, because a view collection is addressed by it.
//!
//! A **multi** relation becomes a correlated `jsonb_agg` subquery returning an
//! array, mirroring PocketBase's array expansion. A plain join would fan the base
//! row out and break that uniqueness, so it is not an option.
//!
//! Every identifier is validated and quoted, so casing is preserved verbatim.

use serde_json::Value;

use lazypock_core::error::{Error, Result};

use crate::collections::{Collection, Field};
use crate::ddl::quote_ident;
use crate::types::FieldType;

/// Generate the view SQL for `spec`, resolving collections from `collections`.
pub fn to_query(spec: &Value, collections: &[Collection]) -> Result<String> {
    Ok(render(&build_plan(spec, collections)?))
}

/// A validated spec, ready to render.
struct Plan {
    source: String,
    relations: Vec<Relation>,
    fields: Vec<Output>,
    sort: Vec<(Direction, String)>,
    limit: Option<i64>,
}

/// A declared relation, resolved against the provided collections.
struct Relation {
    /// The name the spec refers to it by.
    alias: String,
    /// The relation field on the base collection.
    field: String,
    /// The target collection's name.
    target: String,
    /// Whether the field can hold more than one target row.
    multi: bool,
}

/// Where an output column comes from.
enum Origin {
    /// A column on the base collection.
    Base,
    /// A column on a declared relation, by its index in [`Plan::relations`].
    Related(usize),
}

/// One selected column.
struct Output {
    origin: Origin,
    /// The column name on the collection it comes from.
    name: String,
    /// The name it is emitted as, after de-duplication.
    alias: String,
}

#[derive(Clone, Copy)]
enum Direction {
    Asc,
    Desc,
}

/// A column chosen for output.
///
/// Only the name is needed: the type is whatever Postgres resolves the expression
/// to when the view is created, and a `select` cannot lie about it.
struct Column {
    name: String,
}

fn build_plan(spec: &Value, collections: &[Collection]) -> Result<Plan> {
    if !spec.is_object() {
        return Err(invalid("the view builder spec must be an object"));
    }

    let source = fetch_collection(collections, spec.get("source"), "source collection")?;
    let relations = build_relations(spec.get("relations"), source, collections)?;
    let fields = build_fields(spec.get("fields"), source, &relations, collections)?;
    let fields = uniquify_aliases(ensure_id(fields));
    let sort = build_sort(spec.get("sort"), source)?;
    let limit = build_limit(spec.get("limit"))?;

    Ok(Plan {
        source: source.name.clone(),
        relations,
        fields,
        sort,
        limit,
    })
}

/// Find a collection by name.
fn find<'a>(collections: &'a [Collection], name: &str) -> Option<&'a Collection> {
    collections
        .iter()
        .find(|collection| collection.name == name)
}

/// Read a required collection name and resolve it.
fn fetch_collection<'a>(
    collections: &'a [Collection],
    value: Option<&Value>,
    label: &str,
) -> Result<&'a Collection> {
    let name = value.and_then(Value::as_str).unwrap_or("");
    if name.is_empty() {
        return Err(invalid(format!("{label} is required")));
    }
    find(collections, name).ok_or_else(|| invalid(format!("{label} '{name}' does not exist")))
}

// ── Relations ───────────────────────────────────────────────────────────────

fn build_relations(
    value: Option<&Value>,
    source: &Collection,
    collections: &[Collection],
) -> Result<Vec<Relation>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(invalid("relations must be a list"));
    };

    let mut relations: Vec<Relation> = Vec::with_capacity(items.len());
    for item in items {
        let alias = validate_identifier(item.get("alias"), "relation alias")?;
        if alias == source.name {
            return Err(invalid(format!(
                "relation alias '{alias}' conflicts with the source collection name"
            )));
        }
        if relations.iter().any(|relation| relation.alias == alias) {
            return Err(invalid(format!("duplicate relation alias '{alias}'")));
        }

        let field = fetch_relation_field(source, item.get("field"))?;
        let target = relation_target(field)?;
        if find(collections, &target).is_none() {
            return Err(invalid(format!(
                "relation target '{target}' does not exist"
            )));
        }

        relations.push(Relation {
            alias,
            field: field.name.clone(),
            target,
            multi: is_multi_relation(field),
        });
    }

    Ok(relations)
}

/// The relation field the spec named, which must exist and must be a relation.
fn fetch_relation_field<'a>(source: &'a Collection, value: Option<&Value>) -> Result<&'a Field> {
    let name = value.and_then(Value::as_str).unwrap_or("");
    let field = source
        .fields
        .iter()
        .find(|field| field.name == name)
        .ok_or_else(|| {
            invalid(format!(
                "relation field '{name}' does not exist on '{}'",
                source.name
            ))
        })?;

    if field.field_type != FieldType::Relation {
        return Err(invalid(format!(
            "field '{}' on '{}' is not a relation",
            field.name, source.name
        )));
    }
    Ok(field)
}

/// The collection a relation field points at.
fn relation_target(field: &Field) -> Result<String> {
    field
        .options
        .get("collection")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            invalid(format!(
                "relation field '{}' has no target collection",
                field.name
            ))
        })
}

/// Whether a relation holds more than one row, which decides join or aggregate.
fn is_multi_relation(field: &Field) -> bool {
    field
        .options
        .get("maxSelect")
        .and_then(Value::as_i64)
        .is_some_and(|max| max > 1)
}

// ── Fields ──────────────────────────────────────────────────────────────────

fn build_fields(
    value: Option<&Value>,
    source: &Collection,
    relations: &[Relation],
    collections: &[Collection],
) -> Result<Vec<Output>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(invalid("fields must be a list"));
    };

    items
        .iter()
        .map(|item| build_field(item, source, relations, collections))
        .collect()
}

fn build_field(
    item: &Value,
    source: &Collection,
    relations: &[Relation],
    collections: &[Collection],
) -> Result<Output> {
    let field_source = item
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or(source.name.as_str());
    let name = item.get("name").and_then(Value::as_str);

    if field_source == source.name {
        let column = fetch_column(source, name, "field")?;
        let alias = output_alias(item.get("as"), &column.name)?;
        return Ok(Output {
            origin: Origin::Base,
            name: column.name,
            alias,
        });
    }

    let Some(index) = relations
        .iter()
        .position(|relation| relation.alias == field_source)
    else {
        return Err(invalid(format!("unknown field source '{field_source}'")));
    };

    let relation = &relations[index];
    let target = find(collections, &relation.target).ok_or_else(|| {
        invalid(format!(
            "relation target '{}' does not exist",
            relation.target
        ))
    })?;
    let column = fetch_column(target, name, "related field")?;
    // A related column defaults to a name that says where it came from, so
    // `customer.name` does not silently shadow the base `name`.
    let default = format!("{}_{}", relation.field, column.name);
    let alias = output_alias(item.get("as"), &default)?;

    Ok(Output {
        origin: Origin::Related(index),
        name: column.name,
        alias,
    })
}

/// A column the spec may select, or the reason it may not.
fn fetch_column(collection: &Collection, name: Option<&str>, label: &str) -> Result<Column> {
    let Some(name) = name else {
        return Err(invalid(format!(
            "{label} name is required on '{}'",
            collection.name
        )));
    };

    // The built-in `id` is always selectable and is not in `fields`.
    if name == "id" {
        return Ok(Column {
            name: "id".to_string(),
        });
    }

    let Some(field) = collection.fields.iter().find(|field| field.name == name) else {
        return Err(invalid(format!(
            "{label} '{name}' does not exist on '{}'",
            collection.name
        )));
    };

    // A hidden field is hidden from the API, so it must not be reachable by
    // building a view either.
    if field
        .options
        .get("hidden")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(invalid(format!("{label} '{name}' is hidden")));
    }
    if field.field_type == FieldType::Password {
        return Err(invalid(format!("{label} '{name}' is a password field")));
    }

    Ok(Column {
        name: field.name.clone(),
    })
}

/// The `id` column is injected when the spec does not ask for it.
///
/// A view is addressed by its `id`, so the view has to carry the base row's. If
/// another column also claims the name `id`, the base one is still added — the
/// de-duplication below renames the other, not this.
fn ensure_id(mut fields: Vec<Output>) -> Vec<Output> {
    let present = fields.iter().any(|field| {
        matches!(field.origin, Origin::Base) && field.name == "id" && field.alias == "id"
    });
    if !present {
        fields.insert(
            0,
            Output {
                origin: Origin::Base,
                name: "id".to_string(),
                alias: "id".to_string(),
            },
        );
    }
    fields
}

/// Rename output collisions: `name`, `name_2`, `name_3`, …
fn uniquify_aliases(mut fields: Vec<Output>) -> Vec<Output> {
    let mut used: Vec<String> = Vec::with_capacity(fields.len());
    for field in fields.iter_mut() {
        let base = field.alias.clone();
        let mut candidate = base.clone();
        let mut suffix = 2u32;
        while used.contains(&candidate) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        used.push(candidate.clone());
        field.alias = candidate;
    }
    fields
}

/// The output name: what the spec asked for, or the default.
fn output_alias(value: Option<&Value>, default: &str) -> Result<String> {
    match value {
        // An empty name means "not given", the same as omitting it.
        None | Some(Value::Null) => validate_name(default, "output column name"),
        Some(Value::String(name)) if !name.is_empty() => validate_name(name, "output column name"),
        Some(Value::String(_)) => validate_name(default, "output column name"),
        Some(_) => Err(invalid("output column name must be a string")),
    }
}

// ── Sort and limit ──────────────────────────────────────────────────────────

fn build_sort(value: Option<&Value>, source: &Collection) -> Result<Vec<(Direction, String)>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let Some(sort) = value.as_str() else {
        return Err(invalid("sort must be a string"));
    };

    let mut out = Vec::new();
    for token in sort.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let (direction, name) = match token.strip_prefix('-') {
            Some(rest) => (Direction::Desc, rest),
            None => (Direction::Asc, token.strip_prefix('+').unwrap_or(token)),
        };
        let column = fetch_column(source, Some(name.trim()), "sort field")?;
        out.push((direction, column.name));
    }
    Ok(out)
}

fn build_limit(value: Option<&Value>) -> Result<Option<i64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => match number.as_i64() {
            Some(limit) if limit > 0 => Ok(Some(limit)),
            _ => Err(invalid("limit must be a positive integer")),
        },
        Some(_) => Err(invalid("limit must be a positive integer")),
    }
}

// ── Rendering ───────────────────────────────────────────────────────────────

fn render(plan: &Plan) -> String {
    let source = &plan.source;

    let select: Vec<String> = plan
        .fields
        .iter()
        .map(|field| {
            format!(
                "    {} AS {}",
                select_expr(field, plan),
                quote_ident(&field.alias)
            )
        })
        .collect();

    let mut lines = vec!["SELECT".to_string()];
    let last = select.len().saturating_sub(1);
    for (index, line) in select.iter().enumerate() {
        let comma = if index == last { "" } else { "," };
        lines.push(format!("{line}{comma}"));
    }
    lines.push(format!("FROM {}", quote_ident(source)));

    // Only single relations produce a join; a multi one is aggregated in the
    // select list instead, so there is no row to fan out.
    for relation in plan.relations.iter().filter(|relation| !relation.multi) {
        lines.push(format!(
            "LEFT JOIN {target} AS {alias} ON {alias}.{id}::text = {source}.{field}",
            target = quote_ident(&relation.target),
            alias = quote_ident(&relation.alias),
            id = quote_ident("id"),
            source = quote_ident(source),
            field = quote_ident(&relation.field),
        ));
    }

    if !plan.sort.is_empty() {
        let body = plan
            .sort
            .iter()
            .map(|(direction, name)| {
                let direction = match direction {
                    Direction::Asc => "ASC",
                    Direction::Desc => "DESC",
                };
                format!("{}.{} {direction}", quote_ident(source), quote_ident(name))
            })
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("ORDER BY {body}"));
    }

    if let Some(limit) = plan.limit {
        lines.push(format!("LIMIT {limit}"));
    }

    lines.join("\n")
}

/// One output column as an expression.
fn select_expr(field: &Output, plan: &Plan) -> String {
    match field.origin {
        Origin::Base => format!("{}.{}", quote_ident(&plan.source), quote_ident(&field.name)),
        Origin::Related(index) => {
            let relation = &plan.relations[index];
            if !relation.multi {
                return format!(
                    "{}.{}",
                    quote_ident(&relation.alias),
                    quote_ident(&field.name)
                );
            }
            // A multi relation is an array, so it becomes an aggregate over the
            // target rather than a join. `_agg` is fixed rather than the declared
            // alias, because this is a fresh scope with only one table in it.
            let inner = quote_ident("_agg");
            let id = format!("{inner}.{}", quote_ident("id"));
            let column = format!("{inner}.{}", quote_ident(&field.name));
            let target = quote_ident(&relation.target);
            let base = format!(
                "{}.{}",
                quote_ident(&plan.source),
                quote_ident(&relation.field)
            );
            format!(
                "(SELECT COALESCE(jsonb_agg({column} ORDER BY {id}), '[]'::jsonb) \
                 FROM {target} AS {inner} WHERE {id}::text = ANY({base}))"
            )
        }
    }
}

// ── Small helpers ───────────────────────────────────────────────────────────

/// Check an identifier, or explain what an identifier has to look like.
fn validate_identifier(value: Option<&Value>, label: &str) -> Result<String> {
    let Some(name) = value.and_then(Value::as_str) else {
        return Err(invalid(format!("{label} is required")));
    };
    validate_name(name, label)
}

/// Check a name that is already known to be a string.
fn validate_name(name: &str, label: &str) -> Result<String> {
    if !is_identifier(name) {
        return Err(invalid(format!(
            "{label} '{name}' must start with a letter or underscore and contain only letters, \
             digits, and underscores"
        )));
    }
    Ok(name.to_string())
}

/// Whether a name is safe to quote as an identifier.
///
/// Not a security boundary — everything goes through [`quote_ident`] — but a name
/// that cannot be written plainly would be a confusing thing to create.
fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn invalid(message: impl Into<String>) -> Error {
    Error::invalid(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CollectionType;
    use serde_json::json;
    use std::time::SystemTime;

    fn field(name: &str, field_type: FieldType) -> Field {
        Field {
            id: String::new(),
            collection_id: String::new(),
            name: name.to_string(),
            field_type,
            required: false,
            is_unique: false,
            options: json!({}),
            position: 0,
        }
    }

    fn text(name: &str) -> Field {
        field(name, FieldType::Text)
    }

    fn relation(name: &str, target: &str, max_select: i64) -> Field {
        let mut field = field(name, FieldType::Relation);
        field.options = json!({ "collection": target, "maxSelect": max_select });
        field
    }

    fn collection(name: &str, fields: Vec<Field>) -> Collection {
        Collection {
            id: String::new(),
            name: name.to_string(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules: json!({}),
            fields,
            created: SystemTime::now(),
            updated: SystemTime::now(),
        }
    }

    /// Join `lines` with newlines, the way the reference tests pin the SQL.
    fn lines(lines: &[&str]) -> String {
        lines.join("\n")
    }

    fn query(spec: Value, collections: &[Collection]) -> String {
        to_query(&spec, collections).expect("the spec is valid")
    }

    fn refusal(spec: Value, collections: &[Collection]) -> String {
        to_query(&spec, collections)
            .expect_err("the spec should be refused")
            .message()
            .to_string()
    }

    #[test]
    fn injects_the_id_column_and_preserves_casing_verbatim() {
        let orders = collection(
            "orders",
            vec![
                field("is_active", FieldType::Bool),
                field("isActive", FieldType::Bool),
            ],
        );
        let spec = json!({
            "source": "orders",
            "fields": [{"name": "is_active"}, {"name": "isActive"}],
        });

        assert_eq!(
            query(spec, &[orders]),
            lines(&[
                "SELECT",
                "    \"orders\".\"id\" AS \"id\",",
                "    \"orders\".\"is_active\" AS \"is_active\",",
                "    \"orders\".\"isActive\" AS \"isActive\"",
                "FROM \"orders\"",
            ])
        );
    }

    #[test]
    fn does_not_duplicate_an_explicitly_selected_id() {
        let orders = collection("orders", vec![text("title")]);
        let spec = json!({
            "source": "orders",
            "fields": [{"name": "id"}, {"name": "title"}],
        });

        assert_eq!(
            query(spec, &[orders]),
            lines(&[
                "SELECT",
                "    \"orders\".\"id\" AS \"id\",",
                "    \"orders\".\"title\" AS \"title\"",
                "FROM \"orders\"",
            ])
        );
    }

    /// The view is addressed by its id, so the base one always survives — the
    /// column that also claimed the name is the one that gets renamed.
    #[test]
    fn always_keeps_the_base_id_even_when_another_field_claims_the_name() {
        let orders = collection("orders", vec![relation("customer", "customers", 1)]);
        let customers = collection("customers", vec![text("name")]);
        let spec = json!({
            "source": "orders",
            "relations": [{"alias": "t1", "field": "customer"}],
            "fields": [{"source": "t1", "name": "name", "as": "id"}],
        });

        let sql = query(spec, &[orders, customers]);
        assert!(sql.contains("\"orders\".\"id\" AS \"id\""), "{sql}");
        assert!(sql.contains("\"t1\".\"name\" AS \"id_2\""), "{sql}");
    }

    #[test]
    fn a_single_relation_becomes_a_left_join_with_a_text_cast_id_match() {
        let orders = collection("orders", vec![relation("customer", "customers", 1)]);
        let customers = collection("customers", vec![text("name"), text("email")]);
        let spec = json!({
            "source": "orders",
            "relations": [{"alias": "t1", "field": "customer"}],
            "fields": [
                {"source": "orders", "name": "id"},
                {"source": "t1", "name": "name"},
            ],
        });

        assert_eq!(
            query(spec, &[orders, customers]),
            lines(&[
                "SELECT",
                "    \"orders\".\"id\" AS \"id\",",
                "    \"t1\".\"name\" AS \"customer_name\"",
                "FROM \"orders\"",
                "LEFT JOIN \"customers\" AS \"t1\" ON \"t1\".\"id\"::text = \"orders\".\"customer\"",
            ])
        );
    }

    /// A plain join would fan the base row out and break the unique id.
    #[test]
    fn a_multi_relation_becomes_a_jsonb_agg_subquery_and_emits_no_join() {
        let orders = collection("orders", vec![relation("authors", "authors", 3)]);
        let authors = collection("authors", vec![text("name")]);
        let spec = json!({
            "source": "orders",
            "relations": [{"alias": "t2", "field": "authors"}],
            "fields": [{"source": "t2", "name": "name"}],
        });

        let sql = query(spec, &[orders, authors]);
        assert_eq!(
            sql,
            lines(&[
                "SELECT",
                "    \"orders\".\"id\" AS \"id\",",
                "    (SELECT COALESCE(jsonb_agg(\"_agg\".\"name\" ORDER BY \"_agg\".\"id\"), '[]'::jsonb) FROM \"authors\" AS \"_agg\" WHERE \"_agg\".\"id\"::text = ANY(\"orders\".\"authors\")) AS \"authors_name\"",
                "FROM \"orders\"",
            ])
        );
        assert!(!sql.contains("LEFT JOIN"), "{sql}");
    }

    #[test]
    fn honors_a_custom_output_alias() {
        let orders = collection("orders", vec![relation("customer", "customers", 1)]);
        let customers = collection("customers", vec![text("name")]);
        let spec = json!({
            "source": "orders",
            "relations": [{"alias": "t1", "field": "customer"}],
            "fields": [{"source": "t1", "name": "name", "as": "authorName"}],
        });

        assert!(query(spec, &[orders, customers]).contains("\"t1\".\"name\" AS \"authorName\""));
    }

    #[test]
    fn supports_a_self_relation() {
        let orders = collection("orders", vec![relation("parent", "orders", 1)]);
        let spec = json!({
            "source": "orders",
            "relations": [{"alias": "t1", "field": "parent"}],
            "fields": [{"source": "t1", "name": "id", "as": "parent_id"}],
        });

        assert!(query(spec, &[orders]).contains(
            "LEFT JOIN \"orders\" AS \"t1\" ON \"t1\".\"id\"::text = \"orders\".\"parent\""
        ));
    }

    #[test]
    fn de_duplicates_output_names_deterministically() {
        let orders = collection(
            "orders",
            vec![text("customer_name"), relation("customer", "customers", 1)],
        );
        let customers = collection("customers", vec![text("name")]);
        let spec = json!({
            "source": "orders",
            "relations": [{"alias": "t1", "field": "customer"}],
            "fields": [{"name": "customer_name"}, {"source": "t1", "name": "name"}],
        });

        let sql = query(spec, &[orders, customers]);
        assert!(
            sql.contains("\"orders\".\"customer_name\" AS \"customer_name\""),
            "{sql}"
        );
        assert!(
            sql.contains("\"t1\".\"name\" AS \"customer_name_2\""),
            "{sql}"
        );
    }

    #[test]
    fn qualifies_base_columns_and_honors_direction() {
        let orders = collection(
            "orders",
            vec![field("created_at", FieldType::Date), text("name")],
        );
        let spec = json!({
            "source": "orders",
            "fields": [{"name": "name"}],
            "sort": "-created_at, name",
            "limit": 50,
        });

        assert_eq!(
            query(spec, &[orders]),
            lines(&[
                "SELECT",
                "    \"orders\".\"id\" AS \"id\",",
                "    \"orders\".\"name\" AS \"name\"",
                "FROM \"orders\"",
                "ORDER BY \"orders\".\"created_at\" DESC, \"orders\".\"name\" ASC",
                "LIMIT 50",
            ])
        );
    }

    #[test]
    fn omits_order_by_and_limit_when_not_provided() {
        let orders = collection("orders", vec![text("name")]);
        let spec = json!({ "source": "orders", "fields": [{"name": "name"}] });

        let sql = query(spec, &[orders]);
        assert!(!sql.contains("ORDER BY"), "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
    }

    // ── Refusals ────────────────────────────────────────────────────────────

    #[test]
    fn requires_a_source_collection() {
        assert_eq!(refusal(json!({}), &[]), "source collection is required");
    }

    #[test]
    fn rejects_an_unknown_source_collection() {
        assert!(refusal(json!({"source": "nope"}), &[]).contains("does not exist"));
    }

    #[test]
    fn rejects_an_unknown_base_field() {
        let orders = collection("orders", vec![text("title")]);
        let spec = json!({ "source": "orders", "fields": [{"name": "nope"}] });
        assert!(refusal(spec, &[orders]).contains("does not exist"));
    }

    #[test]
    fn rejects_a_hidden_field() {
        let mut secret = text("secret");
        secret.options = json!({ "hidden": true });
        let orders = collection("orders", vec![secret]);
        let spec = json!({ "source": "orders", "fields": [{"name": "secret"}] });
        assert!(refusal(spec, &[orders]).contains("is hidden"));
    }

    #[test]
    fn rejects_a_non_relation_field_used_as_a_relation() {
        let orders = collection("orders", vec![text("title")]);
        let spec = json!({
            "source": "orders",
            "relations": [{"alias": "t1", "field": "title"}],
        });
        assert!(refusal(spec, &[orders]).contains("is not a relation"));
    }

    #[test]
    fn rejects_a_relation_whose_target_is_missing() {
        let orders = collection("orders", vec![relation("customer", "customers", 1)]);
        let spec = json!({
            "source": "orders",
            "relations": [{"alias": "t1", "field": "customer"}],
        });
        assert!(refusal(spec, &[orders]).contains("relation target"));
    }

    #[test]
    fn rejects_an_unknown_field_source() {
        let orders = collection("orders", vec![text("title")]);
        let spec = json!({
            "source": "orders",
            "fields": [{"source": "nope", "name": "title"}],
        });
        assert!(refusal(spec, &[orders]).contains("unknown field source"));
    }

    #[test]
    fn rejects_an_invalid_relation_alias() {
        let orders = collection("orders", vec![relation("customer", "customers", 1)]);
        let customers = collection("customers", vec![text("name")]);
        let spec = json!({
            "source": "orders",
            "relations": [{"alias": "1 bad", "field": "customer"}],
        });
        assert!(refusal(spec, &[orders, customers]).contains("relation alias"));
    }

    #[test]
    fn rejects_a_relation_alias_that_collides_with_the_source_name() {
        let orders = collection("orders", vec![relation("customer", "customers", 1)]);
        let customers = collection("customers", vec![text("name")]);
        let spec = json!({
            "source": "orders",
            "relations": [{"alias": "orders", "field": "customer"}],
        });
        assert!(refusal(spec, &[orders, customers]).contains("conflicts with the source"));
    }

    #[test]
    fn rejects_duplicate_relation_aliases() {
        let orders = collection(
            "orders",
            vec![relation("a", "customers", 1), relation("b", "customers", 1)],
        );
        let customers = collection("customers", vec![text("name")]);
        let spec = json!({
            "source": "orders",
            "relations": [
                {"alias": "t1", "field": "a"},
                {"alias": "t1", "field": "b"},
            ],
        });
        assert!(refusal(spec, &[orders, customers]).contains("duplicate relation alias"));
    }

    #[test]
    fn rejects_an_invalid_output_alias() {
        let orders = collection("orders", vec![text("title")]);
        let spec = json!({
            "source": "orders",
            "fields": [{"name": "title", "as": "not a name"}],
        });
        assert!(refusal(spec, &[orders]).contains("output column name"));
    }

    #[test]
    fn rejects_a_sort_on_an_unknown_field() {
        let orders = collection("orders", vec![text("title")]);
        let spec = json!({ "source": "orders", "sort": "-nope" });
        assert!(refusal(spec, &[orders]).contains("sort field"));
    }

    #[test]
    fn rejects_invalid_limits() {
        let orders = collection("orders", vec![text("title")]);
        for limit in [json!(0), json!(-1), json!("ten"), json!(1.5)] {
            let spec = json!({ "source": "orders", "limit": limit });
            assert!(
                refusal(spec, std::slice::from_ref(&orders)).contains("limit"),
                "a limit of {limit} should be refused"
            );
        }
    }
}
