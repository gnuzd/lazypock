//! DDL generation for user collections.
//!
//! Every identifier is validated upstream ([`crate::types`]) and quoted here, so
//! a name can never escape its statement. Column *types* come from the type
//! mapper and are chosen from a fixed set of literals, never interpolated from
//! user input.

use lazypock_core::error::{Error, Result};
use sha2::{Digest, Sha256};

use crate::collections::{Collection, Field};
use crate::types::{
    self, validate_identifier, CollectionType, AUTH_SYSTEM_COLUMNS, MAX_IDENTIFIER_LEN,
    SYSTEM_COLUMNS,
};

/// Quote an identifier for use in SQL.
///
/// Validated callers cannot produce a quote character, but the escaping is kept
/// so this stays correct if it is ever called with unchecked input.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Deterministic, collision-resistant index name for a column.
///
/// Postgres index names are per-schema and capped at 63 bytes, so long
/// `table_field` pairs are truncated with a content hash suffix.
pub fn index_name(table: &str, column: &str) -> String {
    let base = format!("idx_{table}_{column}");
    if base.len() <= MAX_IDENTIFIER_LEN {
        return base;
    }
    let digest = Sha256::digest(base.as_bytes());
    let suffix: String = digest[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let keep = MAX_IDENTIFIER_LEN - suffix.len() - 1;
    format!("{}_{suffix}", &base[..keep])
}

/// `create table` for a collection, including system columns and single-relation
/// foreign keys.
///
/// `resolve_table` maps a relation field's `collectionId` to a table name.
pub fn create_table_statement(
    collection: &Collection,
    resolve_table: &dyn Fn(&str) -> Option<String>,
) -> Result<String> {
    validate_identifier("collection name", &collection.name)?;
    if collection.collection_type == CollectionType::View {
        return Err(Error::unsupported(
            "create_table_statement does not build views",
        ));
    }

    let mut columns = vec![
        format!("{} text primary key", quote_ident("id")),
        format!(
            "{} timestamptz not null default now()",
            quote_ident("created")
        ),
        format!(
            "{} timestamptz not null default now()",
            quote_ident("updated")
        ),
    ];

    if collection.collection_type == CollectionType::Auth {
        // From the shared definition, never hardcoded: a column the constant lists
        // but this misses is a column the definition advertises and the table lacks.
        for (name, definition) in types::auth_system_column_definitions() {
            columns.push(format!("{} {definition}", quote_ident(name)));
        }
    }

    for field in &collection.fields {
        columns.push(column_definition(field, resolve_table)?);
    }

    Ok(format!(
        "create table {} (\n    {}\n)",
        quote_ident(&collection.name),
        columns.join(",\n    ")
    ))
}

fn column_definition(
    field: &Field,
    resolve_table: &dyn Fn(&str) -> Option<String>,
) -> Result<String> {
    let column = quote_ident(&field.name);
    let column_type = field.column_type()?;
    let mut definition = format!("{column} {column_type}");

    if field.required {
        definition.push_str(" not null");
    }

    // A single relation gets a real foreign key so the database enforces
    // referential integrity. Multi-relations cannot (Postgres has no array
    // element FK) and are validated by the records layer.
    if field.field_type == types::FieldType::Relation && !field.is_multiple() {
        let target_id = types::relation_target(&field.options).ok_or_else(|| {
            Error::invalid(format!(
                "relation field {:?} has no collectionId",
                field.name
            ))
        })?;
        let target = resolve_table(target_id).ok_or_else(|| {
            Error::invalid(format!(
                "relation field {:?} references unknown collection {target_id:?}",
                field.name
            ))
        })?;
        let on_delete = if types::option_bool(&field.options, "cascadeDelete") {
            "on delete cascade"
        } else {
            "on delete set null"
        };
        definition.push_str(&format!(
            " references {} ({}) {on_delete}",
            quote_ident(&target),
            quote_ident("id")
        ));
    }

    Ok(definition)
}

/// Statements to add a column, in order.
///
/// A required column is added as `not null default <zero>` and the default is
/// then dropped: on Postgres 11+ a constant default is stored as metadata, so
/// this does not rewrite the table, and dropping the default means later inserts
/// must supply a value rather than silently getting the placeholder.
pub fn add_column_statements(table: &str, field: &Field) -> Result<Vec<String>> {
    validate_identifier("collection name", table)?;
    let target = quote_ident(table);
    let column = quote_ident(&field.name);
    let column_type = field.column_type()?;

    if !field.required {
        return Ok(vec![format!(
            "alter table {target} add column {column} {column_type}"
        )]);
    }

    let Some(zero) = types::zero_default(field.field_type, &field.options) else {
        return Err(Error::invalid(format!(
            "cannot add the required field {:?}: a required {} field has no safe placeholder value. \
             Add it as optional, populate it, then set it required.",
            field.name, field.field_type
        )));
    };

    Ok(vec![
        format!("alter table {target} add column {column} {column_type} not null default {zero}"),
        format!("alter table {target} alter column {column} drop default"),
    ])
}

/// `alter table ... drop column`.
pub fn drop_column_statement(table: &str, column: &str) -> Result<String> {
    validate_identifier("collection name", table)?;
    validate_identifier("field name", column)?;
    if SYSTEM_COLUMNS.contains(&column) || AUTH_SYSTEM_COLUMNS.contains(&column) {
        return Err(Error::invalid(format!(
            "{column:?} is a built-in column and cannot be dropped"
        )));
    }
    Ok(format!(
        "alter table {} drop column {}",
        quote_ident(table),
        quote_ident(column)
    ))
}

/// `alter table ... alter column ... type ...`, with an explicit cast.
pub fn alter_column_type_statement(table: &str, column: &str, column_type: &str) -> Result<String> {
    validate_identifier("collection name", table)?;
    validate_identifier("field name", column)?;
    if !is_known_column_type(column_type) {
        return Err(Error::invalid(format!(
            "refusing to alter a column to unknown type {column_type:?}"
        )));
    }
    let target = quote_ident(table);
    let column_quoted = quote_ident(column);
    Ok(format!(
        "alter table {target} alter column {column_quoted} type {column_type} \
         using {column_quoted}::{column_type}"
    ))
}

/// `alter table ... rename column ... to ...`.
pub fn rename_column_statement(table: &str, from: &str, to: &str) -> Result<String> {
    validate_identifier("collection name", table)?;
    validate_identifier("field name", from)?;
    validate_identifier("field name", to)?;
    Ok(format!(
        "alter table {} rename column {} to {}",
        quote_ident(table),
        quote_ident(from),
        quote_ident(to)
    ))
}

/// `create unique index` for a column.
pub fn create_unique_index_statement(table: &str, column: &str) -> String {
    format!(
        "create unique index {} on {} ({})",
        quote_ident(&index_name(table, column)),
        quote_ident(table),
        quote_ident(column)
    )
}

/// The case-insensitive unique email index every auth collection needs.
///
/// Login looks an account up by `lower(email)`, so a duplicate would make that
/// lookup ambiguous — and would let two accounts share one address. The index is
/// partial so several accounts may have no email at all, which is what an
/// OAuth-only account looks like.
pub fn create_auth_email_index_statement(table: &str) -> String {
    format!(
        "create unique index {} on {} (lower(\"email\")) where \"email\" is not null",
        quote_ident(&index_name(table, "email")),
        quote_ident(table)
    )
}

/// The trigger that turns a collection's changes into realtime events.
pub fn create_realtime_trigger_statement(table: &str) -> Result<String> {
    validate_identifier("collection name", table)?;
    Ok(format!(
        "create trigger lazypock_change after insert or update or delete on {} \
         for each row execute function lazypock_change_event()",
        quote_ident(table)
    ))
}

/// `drop index if exists`.
pub fn drop_index_statement(table: &str, column: &str) -> String {
    format!(
        "drop index if exists {}",
        quote_ident(&index_name(table, column))
    )
}

/// `drop table if exists ... cascade`.
///
/// `cascade` is what lets a collection another collection's relation points at be
/// deleted: Postgres drops the referencing foreign key, not the referencing table.
/// Without it the drop fails on the dependant, which `main` did not do — it dropped
/// with `cascade` — and the Studio's delete has no way to resolve (it sends no
/// `cascade`/`force` parameter).
pub fn drop_table_statement(table: &str) -> Result<String> {
    validate_identifier("collection name", table)?;
    Ok(format!(
        "drop table if exists {} cascade",
        quote_ident(table)
    ))
}

/// Column types this module is allowed to emit.
fn is_known_column_type(column_type: &str) -> bool {
    matches!(
        column_type,
        "text" | "bigint" | "double precision" | "boolean" | "timestamptz" | "text[]" | "jsonb"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{CreateCollection, CreateField};
    use crate::types::CollectionType;
    use serde_json::json;

    fn field(name: &str, field_type: &str, options: serde_json::Value) -> Field {
        Field {
            id: "f1".into(),
            collection_id: "c1".into(),
            name: name.into(),
            field_type: types::FieldType::parse(field_type).unwrap(),
            required: false,
            is_unique: false,
            options,
            position: 0,
        }
    }

    fn coll(name: &str, collection_type: CollectionType, fields: Vec<Field>) -> Collection {
        Collection {
            id: "c1".into(),
            name: name.into(),
            collection_type,
            options: json!({}),
            rules: json!({}),
            fields,
            created: std::time::SystemTime::now(),
            updated: std::time::SystemTime::now(),
        }
    }

    #[test]
    fn quotes_and_escapes_identifiers() {
        assert_eq!(quote_ident("posts"), "\"posts\"");
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn index_names_stay_within_the_postgres_limit() {
        assert_eq!(index_name("posts", "title"), "idx_posts_title");

        let long_table = "a".repeat(60);
        let long_column = "b".repeat(60);
        let name = index_name(&long_table, &long_column);
        assert!(name.len() <= MAX_IDENTIFIER_LEN, "{name} is too long");
        // Deterministic and distinct for different inputs.
        assert_eq!(name, index_name(&long_table, &long_column));
        assert_ne!(name, index_name(&long_column, &long_table));
    }

    #[test]
    fn create_table_includes_system_columns_and_fields() {
        let mut title = field("title", "text", json!({}));
        title.required = true;
        let collection = coll("posts", CollectionType::Base, vec![title]);

        let sql = create_table_statement(&collection, &|_| None).unwrap();
        assert!(sql.starts_with("create table \"posts\" ("), "{sql}");
        assert!(sql.contains("\"id\" text primary key"), "{sql}");
        assert!(
            sql.contains("\"created\" timestamptz not null default now()"),
            "{sql}"
        );
        assert!(
            sql.contains("\"updated\" timestamptz not null default now()"),
            "{sql}"
        );
        assert!(sql.contains("\"title\" text not null"), "{sql}");
    }

    #[test]
    fn auth_collections_get_the_auth_system_columns() {
        let collection = coll("users", CollectionType::Auth, vec![]);
        let sql = create_table_statement(&collection, &|_| None).unwrap();
        for column in AUTH_SYSTEM_COLUMNS {
            assert!(
                sql.contains(&format!("\"{column}\"")),
                "missing {column}: {sql}"
            );
        }
        assert!(
            sql.contains("\"verified\" boolean not null default false"),
            "{sql}"
        );
    }

    #[test]
    fn single_relations_get_a_foreign_key() {
        let mut author = field("author", "relation", json!({"collectionId": "users"}));
        author.required = false;
        let collection = coll("posts", CollectionType::Base, vec![author]);

        let resolve = |id: &str| (id == "users").then(|| "users".to_string());
        let sql = create_table_statement(&collection, &resolve).unwrap();
        assert!(
            sql.contains("\"author\" text references \"users\" (\"id\") on delete set null"),
            "{sql}"
        );

        // cascadeDelete switches the FK action.
        let mut cascade = field(
            "author",
            "relation",
            json!({"collectionId": "users", "cascadeDelete": true}),
        );
        cascade.required = false;
        let collection = coll("posts", CollectionType::Base, vec![cascade]);
        let sql = create_table_statement(&collection, &resolve).unwrap();
        assert!(sql.contains("on delete cascade"), "{sql}");
    }

    #[test]
    fn unresolvable_relation_targets_fail_loud() {
        let relation = field("author", "relation", json!({"collectionId": "missing"}));
        let collection = coll("posts", CollectionType::Base, vec![relation]);
        let err = create_table_statement(&collection, &|_| None).unwrap_err();
        assert!(err.message().contains("unknown collection"), "{err}");
    }

    #[test]
    fn required_columns_are_added_with_a_placeholder_default_that_is_then_dropped() {
        let mut required = field("title", "text", json!({}));
        required.required = true;
        let statements = add_column_statements("posts", &required).unwrap();
        assert_eq!(statements.len(), 2);
        assert!(
            statements[0].contains("add column \"title\" text not null default ''"),
            "{}",
            statements[0]
        );
        assert!(
            statements[1].contains("alter column \"title\" drop default"),
            "{}",
            statements[1]
        );

        let optional = field("subtitle", "text", json!({}));
        let statements = add_column_statements("posts", &optional).unwrap();
        assert_eq!(
            statements,
            vec!["alter table \"posts\" add column \"subtitle\" text"]
        );
    }

    #[test]
    fn required_relations_cannot_be_added_to_an_existing_collection() {
        let mut relation = field("author", "relation", json!({"collectionId": "users"}));
        relation.required = true;
        let err = add_column_statements("posts", &relation).unwrap_err();
        assert!(err.message().contains("no safe placeholder"), "{err}");
    }

    #[test]
    fn built_in_columns_cannot_be_dropped() {
        for column in SYSTEM_COLUMNS {
            assert!(drop_column_statement("posts", column).is_err(), "{column}");
        }
        assert!(drop_column_statement("posts", "title").is_ok());
        assert!(drop_column_statement("Posts", "title").is_err());
    }

    #[test]
    fn column_type_changes_only_allow_known_types() {
        assert!(alter_column_type_statement("posts", "views", "bigint").is_ok());
        let err =
            alter_column_type_statement("posts", "views", "text; drop table posts").unwrap_err();
        assert!(err.message().contains("unknown type"), "{err}");
    }

    #[test]
    fn drop_table_is_quoted_and_guarded() {
        // `cascade` so a relation pointing at the deleted collection does not block it.
        assert_eq!(
            drop_table_statement("posts").unwrap(),
            "drop table if exists \"posts\" cascade"
        );
        assert!(drop_table_statement("Posts").is_err());
    }

    #[test]
    fn auth_collections_get_a_case_insensitive_unique_email_index() {
        let statement = create_auth_email_index_statement("users");
        assert!(
            statement.starts_with("create unique index \"idx_users_email\" on \"users\""),
            "{statement}"
        );
        assert!(statement.contains("(lower(\"email\"))"), "{statement}");
        assert!(
            statement.contains("where \"email\" is not null"),
            "{statement}"
        );
    }

    #[test]
    fn collection_validation_rejects_bad_input() {
        let mut create = CreateCollection {
            name: "posts".into(),
            collection_type: CollectionType::Base,
            fields: vec![CreateField {
                name: "title".into(),
                field_type: types::FieldType::Text,
                required: false,
                unique: false,
                options: json!({}),
            }],
            rules: json!({}),
            options: json!({}),
            adopt_existing_table: false,
        };
        // Duplicate field names.
        create.fields.push(CreateField {
            name: "title".into(),
            field_type: types::FieldType::Text,
            required: false,
            unique: false,
            options: json!({}),
        });
        let collection = Collection {
            id: "c1".into(),
            name: create.name.clone(),
            collection_type: create.collection_type,
            options: json!({}),
            rules: json!({}),
            fields: create
                .fields
                .iter()
                .enumerate()
                .map(|(position, field)| Field {
                    id: format!("f{position}"),
                    collection_id: "c1".into(),
                    name: field.name.clone(),
                    field_type: field.field_type,
                    required: field.required,
                    is_unique: field.unique,
                    options: field.options.clone(),
                    position: position as i32,
                })
                .collect(),
            created: std::time::SystemTime::now(),
            updated: std::time::SystemTime::now(),
        };
        let err = collection.validate().unwrap_err();
        assert!(err.message().contains("duplicate field"), "{err}");
    }

    #[test]
    fn password_fields_are_rejected_as_explicit_fields() {
        let field = field("password", "password", json!({}));
        let err = field.validate(CollectionType::Auth).unwrap_err();
        assert!(err.message().contains("built-in auth column"), "{err}");
    }
}
