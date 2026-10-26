//! Building a complete record `SELECT`: projection, `expand`, filter, sort and
//! paging, with Postgres rendering the JSON.
//!
//! Postgres builds the response document (`jsonb_build_object`), so the rows
//! arrive ready to stream and the Rust side never decodes/re-encodes them.
//! Relations are expanded with correlated subqueries rather than joins, which
//! keeps one row per record and avoids row multiplication for multi-relations.
//!
//! Parameters are appended in the order they appear in the statement: the filter
//! is compiled first (so its placeholders are `$1..$n`), then the keyset
//! predicate, then `limit` / `offset`. No placeholder rewriting is needed.

use lazypock_core::error::{Error, Result};

use super::compile::{self, AuthContext, FilterContext};
use super::filter::Expr;
use super::pagination::Page;
use super::projection::{ExpandSpec, FieldSelection};
use super::sort::{self, SortSpec};
use crate::ddl::quote_ident;

/// Maximum rows returned inside one expanded multi-relation.
pub const MAX_EXPAND_ROWS: u32 = 200;
/// Maximum `expand` recursion depth.
pub const MAX_EXPAND_DEPTH: usize = 6;

/// Alias used for the collection's own table in generated statements.
///
/// The filter compiler is given this same alias, so qualified columns and the
/// `from` clause always agree.
const ROOT_ALIAS: &str = "p";

/// A record listing request.
#[derive(Debug, Clone)]
pub struct ReadQuery {
    /// Which keys to return.
    pub selection: FieldSelection,
    /// Which relations to expand.
    pub expand: ExpandSpec,
    /// Optional filter.
    pub filter: Option<Expr>,
    /// Ordering.
    pub sort: SortSpec,
    /// Paging.
    pub page: Page,
}

/// A statement plus its bind parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltQuery {
    /// The SQL.
    pub sql: String,
    /// Positional parameters, matching `$1..$n`.
    pub params: Vec<String>,
}

/// Build the listing statement: one row per record, as a JSON document.
pub fn build_list(
    query: &ReadQuery,
    context: &FilterContext,
    auth: &AuthContext,
) -> Result<BuiltQuery> {
    let table = quote_ident(&context.table);
    let mut params: Vec<String> = Vec::new();
    let mut alias_counter = 0usize;

    // The filter must be compiled before anything else adds parameters, and it
    // must be compiled against the alias this statement actually uses.
    let mut conditions: Vec<String> = Vec::new();
    if let Some(filter) = &query.filter {
        let compiled = compile::compile_with_root(filter, context, auth, ROOT_ALIAS)?;
        conditions.push(compiled.sql);
        params.extend(compiled.params);
    }
    if let Page::Keyset {
        after: Some(cursor),
        ..
    } = &query.page
    {
        conditions.push(cursor.predicate("p", &mut params));
    }
    let where_clause = if conditions.is_empty() {
        "true".to_string()
    } else {
        conditions.join(" and ")
    };

    let document = object_expression(
        context,
        "p",
        &query.selection,
        &query.expand,
        &mut alias_counter,
        0,
    )?;
    let order_by = sort::compile(&query.sort, context)?;

    // The page size and offset are validated integers, but they are still bound
    // as parameters rather than interpolated.
    params.push(query.page.limit().to_string());
    let limit_index = params.len();
    let offset = match &query.page {
        Page::Offset(request) => request.offset(),
        Page::Keyset { .. } => 0,
    };
    params.push(offset.to_string());
    let offset_index = params.len();

    // `keyset_created` / `keyset_id` accompany the document so the caller can mint the
    // next page's cursor without re-reading the row. They are not part of the response
    // body. A view may not expose `created`, and then there is no time-ordered cursor
    // to mint — the caller uses offset paging instead.
    let keyset_created = if context.has_column("created") {
        "p.\"created\"".to_string()
    } else {
        "null::timestamptz".to_string()
    };

    Ok(BuiltQuery {
        sql: format!(
            "select {document} as record, {keyset_created} as keyset_created, p.\"id\" as keyset_id \
             from {table} p \
             where {where_clause} order by {order_by} \
             limit ${limit_index}::text::bigint offset ${offset_index}::text::bigint"
        ),
        params,
    })
}

/// Build the matching `count(*)` statement (no paging).
pub fn build_count(
    filter: Option<&Expr>,
    context: &FilterContext,
    auth: &AuthContext,
) -> Result<BuiltQuery> {
    let table = quote_ident(&context.table);
    let mut params: Vec<String> = Vec::new();
    let where_clause = match filter {
        Some(filter) => {
            let compiled = compile::compile_with_root(filter, context, auth, ROOT_ALIAS)?;
            params.extend(compiled.params);
            compiled.sql
        }
        None => "true".to_string(),
    };

    Ok(BuiltQuery {
        sql: format!("select count(*) from {table} p where {where_clause}"),
        params,
    })
}

/// Build the JSON document expression for one record.
///
/// `root` is the already-quoted reference (table name or alias) the document
/// should be built from. Used for both `SELECT ... FROM <table> <alias>` and
/// `INSERT ... RETURNING`, so the caller must supply whatever the statement
/// actually makes available.
pub fn record_document(
    context: &FilterContext,
    root: &str,
    selection: &FieldSelection,
    expand: &ExpandSpec,
) -> Result<String> {
    let mut alias_counter = 0;
    object_expression(context, root, selection, expand, &mut alias_counter, 0)
}

/// Build the JSON document expression for one level of a record.
fn object_expression(
    context: &FilterContext,
    alias: &str,
    selection: &FieldSelection,
    expand: &ExpandSpec,
    alias_counter: &mut usize,
    depth: usize,
) -> Result<String> {
    if depth > MAX_EXPAND_DEPTH {
        return Err(Error::invalid(format!(
            "`expand` is deeper than the supported {MAX_EXPAND_DEPTH} levels"
        )));
    }

    let mut pairs: Vec<String> = Vec::new();

    for column in selection.effective_columns(context) {
        if !context.has_column(&column) {
            return Err(Error::invalid(format!(
                "unknown field {column:?} in collection {:?}",
                context.table
            )));
        }
        pairs.push(format!(
            "{}, {alias}.{}",
            sql_string_literal(&column),
            quote_ident(&column)
        ));
    }

    // PocketBase's `collectionId` / `collectionName`: constants of the collection, so
    // they cost no join and no extra query. Included with every column (`fields=*`,
    // or no `fields`) or when named, and never over a real column of the same name.
    if !context.collection_id.is_empty() {
        if (selection.all_columns || selection.collection_id) && !context.has_column("collectionId")
        {
            pairs.push(format!(
                "'collectionId', {}",
                sql_string_literal(&context.collection_id)
            ));
        }
        if (selection.all_columns || selection.collection_name)
            && !context.has_column("collectionName")
        {
            pairs.push(format!(
                "'collectionName', {}",
                sql_string_literal(&context.table)
            ));
        }
    }

    let mut expand_pairs: Vec<String> = Vec::new();
    for relation_name in expand.relations_to_expand(context) {
        let relation = context.relations.get(relation_name).ok_or_else(|| {
            Error::invalid(format!(
                "{relation_name:?} is not a relation field of collection {:?}",
                context.table
            ))
        })?;

        *alias_counter += 1;
        let child_alias = format!("r{alias_counter}");

        // A nested `fields` entry narrows the expanded record; without one, the
        // whole related record is returned.
        let child_selection = selection
            .nested_for(relation_name)
            .cloned()
            .unwrap_or_else(FieldSelection::all);

        // `expand=*` expands one level only; going deeper is always explicit.
        let child_expand = if expand.all {
            ExpandSpec::none()
        } else {
            expand
                .nested_for(relation_name)
                .cloned()
                .unwrap_or_default()
        };

        let child_document = object_expression(
            &relation.context,
            &child_alias,
            &child_selection,
            &child_expand,
            alias_counter,
            depth + 1,
        )?;

        let table = quote_ident(&relation.table);
        let column = quote_ident(&relation.column);

        let expression = if relation.multiple {
            // `jsonb_agg(... order by ...)` keeps the array ordered; the inner
            // limit bounds how many related rows are materialized.
            format!(
                "(select coalesce(jsonb_agg(x.item order by x.created desc), '[]'::jsonb) \
                 from (select {child_document} as item, {child_alias}.\"created\" as created \
                 from {table} {child_alias} \
                 where {child_alias}.\"id\" = any({alias}.{column}) \
                 order by {child_alias}.\"created\" desc \
                 limit {MAX_EXPAND_ROWS}) x)"
            )
        } else {
            format!(
                "(select {child_document} from {table} {child_alias} \
                 where {child_alias}.\"id\" = {alias}.{column} limit 1)"
            )
        };

        expand_pairs.push(format!(
            "{}, {expression}",
            sql_string_literal(relation_name)
        ));
    }

    // Expanded relations live under `expand`, and the relation's own key keeps the
    // id — PocketBase's shape, and what the Studio reads (`record.expand[field]`).
    // Writing the object at the relation's key replaced the id and hid the
    // expansion from the Studio, which then rendered the raw object.
    if !expand_pairs.is_empty() {
        pairs.push(format!(
            "'expand', jsonb_build_object({})",
            expand_pairs.join(", ")
        ));
    }

    if pairs.is_empty() {
        return Ok("'{}'::jsonb".to_string());
    }
    Ok(format!("jsonb_build_object({})", pairs.join(", ")))
}

/// A SQL string literal for a JSON key.
fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::super::filter;
    use super::*;
    use crate::collections::{Collection, Field};
    use crate::types::{CollectionType, FieldType};
    use serde_json::json;
    use std::time::SystemTime;

    fn field(name: &str, field_type: FieldType, options: serde_json::Value) -> Field {
        Field {
            id: format!("f_{name}"),
            collection_id: "c1".into(),
            name: name.into(),
            field_type,
            required: false,
            is_unique: false,
            options,
            position: 0,
        }
    }

    fn collection(id: &str, name: &str, fields: Vec<Field>) -> Collection {
        Collection {
            id: id.into(),
            name: name.into(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules: json!({}),
            fields,
            created: SystemTime::now(),
            updated: SystemTime::now(),
        }
    }

    fn fixture() -> FilterContext {
        let users = collection(
            "c_users",
            "users",
            vec![field("name", FieldType::Text, json!({}))],
        );
        let posts = collection(
            "c_posts",
            "posts",
            vec![
                field("title", FieldType::Text, json!({})),
                field(
                    "author",
                    FieldType::Relation,
                    json!({"collectionId": "c_users"}),
                ),
                field(
                    "reviewers",
                    FieldType::Relation,
                    json!({"collectionId": "c_users", "maxSelect": 3}),
                ),
            ],
        );
        let all = vec![users, posts.clone()];
        FilterContext::for_collection(&posts, &all).unwrap()
    }

    fn query(
        selection: FieldSelection,
        expand: ExpandSpec,
        filter_input: Option<&str>,
    ) -> ReadQuery {
        ReadQuery {
            selection,
            expand,
            filter: filter_input.map(|input| filter::parse(input).unwrap()),
            sort: SortSpec::default_spec().unwrap(),
            page: Page::from_params(None, None, false, None).unwrap(),
        }
    }

    #[test]
    fn builds_a_minimal_listing() {
        let context = fixture();
        let built = build_list(
            &query(
                FieldSelection::parse(Some("id,title"), &context).unwrap(),
                ExpandSpec::none(),
                None,
            ),
            &context,
            &AuthContext::default(),
        )
        .unwrap();

        assert!(
            built
                .sql
                .contains("jsonb_build_object('id', p.\"id\", 'title', p.\"title\")"),
            "{}",
            built.sql
        );
        assert!(built.sql.contains("from \"posts\" p"));
        assert!(built.sql.contains("where true"));
        assert!(built.sql.contains("order by \"created\" desc"));
        assert!(built
            .sql
            .contains("limit $1::text::bigint offset $2::text::bigint"));
        assert_eq!(built.params, vec!["30", "0"]);
    }

    #[test]
    fn records_carry_the_collection_id_and_name() {
        let context = fixture();
        let sql_for = |fields: Option<&str>| {
            build_list(
                &query(
                    FieldSelection::parse(fields, &context).unwrap(),
                    ExpandSpec::none(),
                    None,
                ),
                &context,
                &AuthContext::default(),
            )
            .unwrap()
            .sql
        };
        let id_literal = format!("'collectionId', '{}'", context.collection_id);
        let name_literal = format!("'collectionName', '{}'", context.table);

        // Every column (no `fields`, or `*`) includes both keys.
        for fields in [None, Some("*")] {
            let sql = sql_for(fields);
            assert!(sql.contains(&id_literal), "{sql}");
            assert!(sql.contains(&name_literal), "{sql}");
        }
        // A narrow projection does not grow them unasked...
        let sql = sql_for(Some("id,title"));
        assert!(
            !sql.contains("'collectionId'") && !sql.contains("'collectionName'"),
            "{sql}"
        );
        // ...but naming them returns exactly those.
        let sql = sql_for(Some("id,collectionName"));
        assert!(
            sql.contains(&name_literal) && !sql.contains("'collectionId'"),
            "{sql}"
        );
    }

    #[test]
    fn filter_parameters_stay_before_the_paging_parameters() {
        let context = fixture();
        let built = build_list(
            &query(
                FieldSelection::parse(Some("id"), &context).unwrap(),
                ExpandSpec::none(),
                Some("title = \"a\" && title ~ \"b\""),
            ),
            &context,
            &AuthContext::default(),
        )
        .unwrap();

        assert_eq!(built.params, vec!["a", "b", "30", "0"]);
        assert!(built.sql.contains("$1::text"));
        assert!(built.sql.contains("$2::text"));
        assert!(built
            .sql
            .contains("limit $3::text::bigint offset $4::text::bigint"));
    }

    #[test]
    fn single_relations_expand_with_a_correlated_subquery() {
        let context = fixture();
        let built = build_list(
            &query(
                FieldSelection::parse(Some("id"), &context).unwrap(),
                ExpandSpec::parse(Some("author"), &context).unwrap(),
                None,
            ),
            &context,
            &AuthContext::default(),
        )
        .unwrap();

        assert!(built.sql.contains("'author', (select "), "{}", built.sql);
        assert!(
            built
                .sql
                .contains("from \"users\" r1 where r1.\"id\" = p.\"author\" limit 1"),
            "{}",
            built.sql
        );
    }

    #[test]
    fn expanded_relations_are_nested_under_expand() {
        let context = fixture();
        let built = build_list(
            &query(
                FieldSelection::parse(Some("id,author"), &context).unwrap(),
                ExpandSpec::parse(Some("author"), &context).unwrap(),
                None,
            ),
            &context,
            &AuthContext::default(),
        )
        .unwrap();

        // The relation's own key keeps the id, and the object goes under `expand` —
        // PocketBase's shape, which is what the Studio reads.
        assert!(
            built.sql.contains("'author', p.\"author\""),
            "{}",
            built.sql
        );
        assert!(
            built
                .sql
                .contains("'expand', jsonb_build_object('author', (select "),
            "{}",
            built.sql
        );
    }

    #[test]
    fn nested_fields_narrow_the_expanded_document() {
        let context = fixture();
        let built = build_list(
            &query(
                FieldSelection::parse(Some("id,author.name"), &context).unwrap(),
                ExpandSpec::parse(Some("author"), &context).unwrap(),
                None,
            ),
            &context,
            &AuthContext::default(),
        )
        .unwrap();

        // The expanded relation only carries the selected key.
        assert!(
            built
                .sql
                .contains("jsonb_build_object('name', r1.\"name\")"),
            "{}",
            built.sql
        );
        assert!(!built.sql.contains("r1.\"created\""), "{}", built.sql);
    }

    #[test]
    fn multi_relations_expand_to_an_ordered_bounded_array() {
        let context = fixture();
        let built = build_list(
            &query(
                FieldSelection::parse(Some("id"), &context).unwrap(),
                ExpandSpec::parse(Some("reviewers"), &context).unwrap(),
                None,
            ),
            &context,
            &AuthContext::default(),
        )
        .unwrap();

        assert!(built.sql.contains("'reviewers', (select coalesce(jsonb_agg(x.item order by x.created desc), '[]'::jsonb)"), "{}", built.sql);
        assert!(
            built.sql.contains("= any(p.\"reviewers\")"),
            "{}",
            built.sql
        );
        assert!(
            built.sql.contains(&format!("limit {MAX_EXPAND_ROWS}")),
            "{}",
            built.sql
        );
    }

    #[test]
    fn expand_wildcard_expands_every_relation_one_level() {
        let context = fixture();
        let built = build_list(
            &query(
                FieldSelection::parse(Some("id"), &context).unwrap(),
                ExpandSpec::parse(Some("*"), &context).unwrap(),
                None,
            ),
            &context,
            &AuthContext::default(),
        )
        .unwrap();

        assert!(built.sql.contains("'author', (select"), "{}", built.sql);
        assert!(built.sql.contains("'reviewers', (select"), "{}", built.sql);
        // One level only: the two top-level relations become r1 and r2, and
        // nothing nested follows.
        assert!(!built.sql.contains("r3."), "{}", built.sql);
    }

    #[test]
    fn nested_expand_recurses() {
        let context = fixture();
        // users has no relations, so ask for a nested expand that does not exist
        // and expect a clear error rather than silence.
        let err = ExpandSpec::parse(Some("author.name"), &context).unwrap_err();
        assert!(err.message().contains("not a relation field"), "{err}");
    }

    #[test]
    fn keyset_paging_adds_a_predicate_and_no_offset() {
        let context = fixture();
        let cursor = super::super::pagination::Cursor {
            created_micros: 1_700_000_000_000_000,
            id: "abcDEF123456789".into(),
        };
        let page = Page::from_params(None, Some(10), false, Some(&cursor.encode())).unwrap();
        let built = build_list(
            &ReadQuery {
                selection: FieldSelection::parse(Some("id"), &context).unwrap(),
                expand: ExpandSpec::none(),
                filter: None,
                sort: SortSpec::default_spec().unwrap(),
                page,
            },
            &context,
            &AuthContext::default(),
        )
        .unwrap();

        assert!(
            built.sql.contains("(p.\"created\", p.\"id\") <"),
            "{}",
            built.sql
        );
        assert_eq!(
            built.params,
            vec!["1700000000000000", "abcDEF123456789", "10", "0"]
        );
        assert!(
            built
                .sql
                .contains("limit $3::text::bigint offset $4::text::bigint"),
            "{}",
            built.sql
        );
    }

    #[test]
    fn reject_unsupported_selector_invalid_field() {
        let context = fixture();
        let mut selection = FieldSelection::parse(Some("id"), &context).unwrap();
        selection.columns.push("nope".into());
        let err = build_list(
            &ReadQuery {
                selection,
                expand: ExpandSpec::none(),
                filter: None,
                sort: SortSpec::default_spec().unwrap(),
                page: Page::from_params(None, None, false, None).unwrap(),
            },
            &context,
            &AuthContext::default(),
        )
        .unwrap_err();
        assert!(err.message().contains("unknown field"), "{err}");
    }

    #[test]
    fn count_uses_the_same_filter_without_paging() {
        let context = fixture();
        let expr = filter::parse("title = \"a\"").unwrap();
        let built = build_count(Some(&expr), &context, &AuthContext::default()).unwrap();
        assert_eq!(
            built.sql,
            "select count(*) from \"posts\" p where p.\"title\" = $1::text"
        );
        assert_eq!(built.params, vec!["a"]);

        let built = build_count(None, &context, &AuthContext::default()).unwrap();
        assert_eq!(built.sql, "select count(*) from \"posts\" p where true");
        assert!(built.params.is_empty());
    }

    #[test]
    fn json_keys_are_escaped() {
        assert_eq!(sql_string_literal("title"), "'title'");
        assert_eq!(sql_string_literal("it's"), "'it''s'");
    }
}
