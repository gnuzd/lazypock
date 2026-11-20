//! Pure SQL generation for table changes (no I/O, easy to unit-test).
//!
//! Structure comes from sea-query (identifiers always quoted). Statements it has
//! no builder for (unique indexes by repo naming, RLS) are assembled from
//! `Ident::quoted()`, whose charset cannot contain a quote.
//!
//! Shape matches the rest of lazypock: `id text primary key` (the application
//! generates ids), `created`/`updated` with `default now()`, uniqueness via a
//! separate `idx_<table>_<column>` index.

use crate::ident::Ident;
use crate::schema::{Collection, Field};
use crate::types::{FieldType, option_bool};
use sea_query::{Alias, ColumnDef, Expr, PostgresQueryBuilder, Table};
use serde_json::Value;
use sha2::{Digest, Sha256};

use lazypock_core::Result;

const MAX_IDENTIFIER_LEN: usize = 63;

/// Deterministic index name; long `table_column` pairs get a hash suffix.
pub fn index_name(table: &str, column: &str) -> String {
    let base = format!("idx_{table}_{column}");
    if base.len() <= MAX_IDENTIFIER_LEN {
        return base;
    }
    let digest = Sha256::digest(base.as_bytes());
    let suffix: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
    let keep = MAX_IDENTIFIER_LEN - suffix.len() - 1;
    format!("{}_{suffix}", &base[..keep])
}

fn typed_column(name: Alias, ty: FieldType, options: &Value) -> Result<ColumnDef> {
    let mut c = ColumnDef::new(name);
    match ty.column_type(options)? {
        "text" => c.text(),
        "bigint" => c.big_integer(),
        "double precision" => c.double(),
        "boolean" => c.boolean(),
        "timestamptz" => c.timestamp_with_time_zone(),
        _ => c.json_binary(),
    };
    Ok(c)
}

fn column(f: &Field) -> Result<ColumnDef> {
    let mut c = typed_column(f.name.alias(), f.ty, &f.options)?;
    if f.required {
        c.not_null();
    }
    Ok(c)
}

/// `create table` plus its indexes, as separate statements.
pub fn create_table(c: &Collection) -> Result<Vec<String>> {
    let mut t = Table::create();
    t.table(c.name.alias())
        .col(
            ColumnDef::new(Alias::new("id"))
                .text()
                .not_null()
                .primary_key(),
        )
        .col(
            ColumnDef::new(Alias::new("created"))
                .timestamp_with_time_zone()
                .not_null()
                .default(Expr::cust("now()")),
        )
        .col(
            ColumnDef::new(Alias::new("updated"))
                .timestamp_with_time_zone()
                .not_null()
                .default(Expr::cust("now()")),
        );

    if c.ty.is_auth() {
        t.col(ColumnDef::new(Alias::new("email")).text())
            .col(ColumnDef::new(Alias::new("password_hash")).text())
            .col(
                ColumnDef::new(Alias::new("verified"))
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .col(ColumnDef::new(Alias::new("token_key")).text())
            .col(
                ColumnDef::new(Alias::new("emailVisibility"))
                    .boolean()
                    .not_null()
                    .default(false),
            );
    }
    for f in &c.fields {
        t.col(column(f)?);
    }

    let mut out = vec![t.to_string(PostgresQueryBuilder)];
    for f in c.fields.iter().filter(|f| f.is_unique) {
        out.push(create_unique_index(&c.name, &f.name));
    }
    if c.ty.is_auth() {
        out.push(create_auth_email_index(&c.name));
    }
    Ok(out)
}

pub fn create_unique_index(table: &Ident, column: &Ident) -> String {
    format!(
        "create unique index \"{}\" on {} ({})",
        index_name(table.as_str(), column.as_str()),
        table.quoted(),
        column.quoted()
    )
}

/// Login looks an account up by `lower(email)`; partial, so accounts without an
/// email (OAuth-only) do not collide.
pub fn create_auth_email_index(table: &Ident) -> String {
    format!(
        "create unique index \"{}\" on {} (lower(\"email\")) where \"email\" is not null",
        index_name(table.as_str(), "email"),
        table.quoted()
    )
}

pub fn drop_unique_index(table: &Ident, column: &Ident) -> String {
    format!(
        "drop index if exists \"{}\"",
        index_name(table.as_str(), column.as_str())
    )
}

pub fn rename_index(table: &Ident, from: &Ident, to: &Ident) -> String {
    format!(
        "alter index if exists \"{}\" rename to \"{}\"",
        index_name(table.as_str(), from.as_str()),
        index_name(table.as_str(), to.as_str())
    )
}

pub fn drop_table(name: &Ident) -> String {
    Table::drop()
        .table(name.alias())
        .to_string(PostgresQueryBuilder)
}

pub fn rename_table(from: &Ident, to: &Ident) -> String {
    Table::rename()
        .table(from.alias(), to.alias())
        .to_string(PostgresQueryBuilder)
}

/// Adding a required column to a table that may have rows: add it with a zero
/// value, then drop the default, so existing rows stay valid.
pub fn add_column(table: &Ident, f: &Field) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut col = typed_column(f.name.alias(), f.ty, &f.options)?;
    if f.required {
        let zero = match f.ty {
            FieldType::Number => "0",
            FieldType::Bool => "false",
            FieldType::Date => "now()",
            FieldType::Json => "'{}'::jsonb",
            _ => "''",
        };
        col.not_null().default(Expr::cust(zero));
        out.push(
            Table::alter()
                .table(table.alias())
                .add_column(col)
                .to_string(PostgresQueryBuilder),
        );
        out.push(format!(
            "alter table {} alter column {} drop default",
            table.quoted(),
            f.name.quoted()
        ));
    } else {
        out.push(
            Table::alter()
                .table(table.alias())
                .add_column(col)
                .to_string(PostgresQueryBuilder),
        );
    }
    if f.is_unique {
        out.push(create_unique_index(table, &f.name));
    }
    let _ = option_bool; // (kept for options-driven DDL later)
    Ok(out)
}

pub fn drop_column(table: &Ident, col: &Ident) -> String {
    Table::alter()
        .table(table.alias())
        .drop_column(col.alias())
        .to_string(PostgresQueryBuilder)
}

pub fn rename_column(table: &Ident, from: &Ident, to: &Ident) -> String {
    Table::alter()
        .table(table.alias())
        .rename_column(from.alias(), to.alias())
        .to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CollectionType;
    use serde_json::json;

    fn id(s: &str) -> Ident {
        Ident::new(s).unwrap()
    }

    #[test]
    fn create_base_table() {
        let mut c = Collection::new("posts", CollectionType::Base).unwrap();
        let mut title = Field::new("title", FieldType::Text).unwrap();
        title.required = true;
        let mut slug = Field::new("slug", FieldType::Text).unwrap();
        slug.is_unique = true;
        let mut views = Field::new("views", FieldType::Number).unwrap();
        views.options = json!({"onlyInt": true});
        c.fields = vec![
            title,
            slug,
            views,
            Field::new("meta", FieldType::Json).unwrap(),
        ];
        let sql = create_table(&c).unwrap();
        assert!(sql[0].starts_with("CREATE TABLE \"posts\""), "{}", sql[0]);
        assert!(sql[0].contains("\"id\" text NOT NULL PRIMARY KEY"));
        assert!(sql[0].contains("\"created\" timestamp with time zone NOT NULL DEFAULT now()"));
        assert!(sql[0].contains("\"title\" text NOT NULL"));
        assert!(sql[0].contains("\"views\" bigint"));
        assert!(sql[0].contains("\"meta\" jsonb"));
        assert_eq!(
            sql[1],
            "create unique index \"idx_posts_slug\" on \"posts\" (\"slug\")"
        );
        assert_eq!(sql.len(), 2);
    }

    #[test]
    fn auth_table_has_system_columns_and_email_index() {
        let c = Collection::new("users", CollectionType::Auth).unwrap();
        let sql = create_table(&c).unwrap();
        for col in [
            "\"email\" text",
            "\"password_hash\" text",
            "\"verified\" bool NOT NULL DEFAULT FALSE",
            "\"token_key\" text",
            "\"emailVisibility\" bool NOT NULL DEFAULT FALSE",
        ] {
            assert!(sql[0].contains(col), "{col} in {}", sql[0]);
        }
        assert!(
            sql.last()
                .unwrap()
                .contains("(lower(\"email\")) where \"email\" is not null")
        );
    }

    #[test]
    fn unsupported_types_cannot_be_created() {
        let mut c = Collection::new("posts", CollectionType::Base).unwrap();
        c.fields = vec![Field::new("r", FieldType::Relation).unwrap()];
        assert!(create_table(&c).is_err());
    }

    #[test]
    fn alter_sql() {
        let mut f = Field::new("n", FieldType::Number).unwrap();
        assert_eq!(
            add_column(&id("t"), &f).unwrap(),
            ["ALTER TABLE \"t\" ADD COLUMN \"n\" double precision"]
        );
        f.required = true;
        let sql = add_column(&id("t"), &f).unwrap();
        assert!(
            sql[0].contains("NOT NULL DEFAULT 0") && sql[1].ends_with("drop default"),
            "{sql:?}"
        );
        assert_eq!(
            drop_column(&id("t"), &id("n")),
            "ALTER TABLE \"t\" DROP COLUMN \"n\""
        );
        assert_eq!(
            rename_column(&id("t"), &id("a"), &id("b")),
            "ALTER TABLE \"t\" RENAME COLUMN \"a\" TO \"b\""
        );
        assert_eq!(
            rename_table(&id("a"), &id("b")),
            "ALTER TABLE \"a\" RENAME TO \"b\""
        );
        assert_eq!(drop_table(&id("a")), "DROP TABLE \"a\"");
    }

    #[test]
    fn long_index_names_are_hashed_and_bounded() {
        let long = "x".repeat(60);
        let name = index_name(&long, "column");
        assert!(name.len() <= MAX_IDENTIFIER_LEN, "{name}");
        assert_eq!(name, index_name(&long, "column"));
        assert_ne!(name, index_name(&long, "other"));
    }
}
