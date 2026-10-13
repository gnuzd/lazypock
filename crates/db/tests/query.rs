//! Integration tests for the query plane: the compiled SQL is executed against
//! a real Postgres, so this also proves the generated statements are valid and
//! that user values are treated as data rather than syntax.
//!
//! Requires a Postgres; see `tests/support/mod.rs`.

mod support;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::ddl::quote_ident;
use lazypock_db::query::compile::{self, AuthContext, FilterContext};
use lazypock_db::query::{filter, sort};
use lazypock_db::tokio_postgres::types::ToSql;
use lazypock_db::tokio_postgres::Client;
use lazypock_db::types::{CollectionType, FieldType};
use serde_json::json;

use support::migrated_test_db;

fn field(name: &str, field_type: FieldType, options: serde_json::Value) -> CreateField {
    CreateField {
        name: name.into(),
        field_type,
        required: false,
        unique: false,
        options,
    }
}

fn collection(name: &str, fields: Vec<CreateField>) -> CreateCollection {
    CreateCollection {
        name: name.into(),
        collection_type: CollectionType::Base,
        fields,
        rules: json!({}),
        options: json!({}),
        adopt_existing_table: false,
    }
}

/// Create the fixture collections and seed deterministic rows.
async fn seed(client: &mut Client) {
    collections::create(
        client,
        collection(
            "users",
            vec![
                field("name", FieldType::Text, json!({})),
                field("age", FieldType::Number, json!({"onlyInt": true})),
            ],
        ),
    )
    .await
    .expect("create users");

    let mut title = field("title", FieldType::Text, json!({}));
    title.required = true;

    collections::create(
        client,
        collection(
            "posts",
            vec![
                title,
                field("views", FieldType::Number, json!({"onlyInt": true})),
                field("rating", FieldType::Number, json!({})),
                field("published", FieldType::Bool, json!({})),
                field(
                    "tags",
                    FieldType::Select,
                    json!({"values": ["a", "b", "c"], "maxSelect": 5}),
                ),
                field(
                    "author",
                    FieldType::Relation,
                    json!({"collectionId": relation_target(client, "users").await}),
                ),
                field(
                    "reviewers",
                    FieldType::Relation,
                    json!({
                        "collectionId": relation_target(client, "users").await,
                        "maxSelect": 3,
                    }),
                ),
            ],
        ),
    )
    .await
    .expect("create posts");

    client
        .batch_execute(
            r#"
            insert into "users" ("id", "name", "age") values
                ('u1', 'Alice', 30),
                ('u2', 'Bob', 40);

            insert into "posts"
                ("id", "title", "views", "rating", "published", "tags", "author", "reviewers")
            values
                ('p1', 'Hello World',     10, 1.5, true,  '{a,b}',   'u1', '{u2}'),
                ('p2', 'Second 50% post', 20, 2.5, false, '{b}',     'u2', '{u1,u2}'),
                ('p3', 'hello again',      5, 3.5, true,  '{}',      'u1', '{}');
            "#,
        )
        .await
        .expect("seed rows");
}

async fn relation_target(client: &Client, name: &str) -> String {
    collections::get(client, name).await.expect("target").id
}

async fn filter_context(client: &Client, name: &str) -> FilterContext {
    let all = collections::list(client).await.expect("list collections");
    let collection = all
        .iter()
        .find(|candidate| candidate.name == name)
        .expect("collection exists");
    FilterContext::for_collection(collection, &all).expect("filter context")
}

async fn select_ids(
    client: &Client,
    table: &str,
    where_sql: &str,
    params: &[String],
    order_by: &str,
) -> Vec<String> {
    let bound: Vec<&(dyn ToSql + Sync)> = params
        .iter()
        .map(|param| param as &(dyn ToSql + Sync))
        .collect();
    let sql = format!(
        "select \"id\" from {} where {} order by {}",
        quote_ident(table),
        where_sql,
        order_by
    );
    client
        .query(&sql, &bound)
        .await
        .unwrap_or_else(|err| panic!("query failed for `{where_sql}`: {err}"))
        .iter()
        .map(|row| row.get::<_, String>(0))
        .collect()
}

/// Compile `input` and run it, returning the matching ids in id order.
async fn matching(
    client: &Client,
    context: &FilterContext,
    input: &str,
    auth: &AuthContext,
) -> Vec<String> {
    let expr = filter::parse(input).unwrap_or_else(|err| panic!("`{input}` should parse: {err}"));
    let compiled = compile::compile(&expr, context, auth)
        .unwrap_or_else(|err| panic!("`{input}` should compile: {err}"));
    select_ids(
        client,
        &context.table,
        &compiled.sql,
        &compiled.params,
        "\"id\" asc",
    )
    .await
}

#[tokio::test]
async fn filters_return_exactly_the_matching_rows() {
    let Some(test) = migrated_test_db("filters_return_matching_rows").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let auth = AuthContext::default();

    let cases: &[(&str, &[&str])] = &[
        ("title = \"Hello World\"", &["p1"]),
        ("title != \"Hello World\"", &["p2", "p3"]),
        // `~` is a case-insensitive contains.
        ("title ~ \"hello\"", &["p1", "p3"]),
        ("title !~ \"hello\"", &["p2"]),
        ("views >= 10", &["p1", "p2"]),
        ("views < 10", &["p3"]),
        ("published = true", &["p1", "p3"]),
        ("published = false", &["p2"]),
        ("rating > 2", &["p2", "p3"]),
        // Required columns are never null.
        ("title = null", &[]),
        ("title != null", &["p1", "p2", "p3"]),
        // Any-of against a `text[]` column.
        ("tags ?= \"a\"", &["p1"]),
        ("tags ?!= \"a\"", &["p2", "p3"]),
        // Logical composition.
        ("published = true && views > 5", &["p1"]),
        ("published = false || views < 10", &["p2", "p3"]),
        // The database clock.
        ("updated < @now", &["p1", "p2", "p3"]),
    ];

    for (input, expected) in cases {
        assert_eq!(
            matching(&client, &context, input, &auth).await,
            expected.to_vec(),
            "filter `{input}`"
        );
    }

    test.cleanup().await;
}

#[tokio::test]
async fn like_wildcards_in_values_are_matched_literally() {
    let Some(test) = migrated_test_db("like_wildcards_are_literal").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let auth = AuthContext::default();

    // The stored title contains a literal "50%".
    assert_eq!(
        matching(&client, &context, "title ~ \"50%\"", &auth).await,
        vec!["p2"]
    );
    // An unescaped `%` would match every row; it must not.
    assert_eq!(
        matching(&client, &context, "title ~ \"%\"", &auth).await,
        vec!["p2"]
    );
    // `_` is literal too: "50_" must not match "50%".
    assert_eq!(
        matching(&client, &context, "title ~ \"50_\"", &auth).await,
        Vec::<String>::new()
    );

    test.cleanup().await;
}

#[tokio::test]
async fn dotted_relation_filters_follow_the_relation() {
    let Some(test) = migrated_test_db("dotted_relation_filters").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let auth = AuthContext::default();

    // Single relation: `author.name`.
    assert_eq!(
        matching(&client, &context, "author.name = \"Alice\"", &auth).await,
        vec!["p1", "p3"]
    );
    assert_eq!(
        matching(&client, &context, "author.name = \"Bob\"", &auth).await,
        vec!["p2"]
    );
    assert_eq!(
        matching(&client, &context, "author.age > 35", &auth).await,
        vec!["p2"]
    );

    // Multi relation: `reviewers.name` (any of them).
    assert_eq!(
        matching(&client, &context, "reviewers.name = \"Alice\"", &auth).await,
        vec!["p2"]
    );
    assert_eq!(
        matching(&client, &context, "reviewers.name = \"Bob\"", &auth).await,
        vec!["p1", "p2"]
    );

    test.cleanup().await;
}

#[tokio::test]
async fn auth_references_resolve_from_the_request_context() {
    let Some(test) = migrated_test_db("auth_references").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;

    let alice = AuthContext {
        id: Some("u1".into()),
        ..Default::default()
    };
    assert_eq!(
        matching(&client, &context, "author = @request.auth.id", &alice).await,
        vec!["p1", "p3"]
    );

    // Unauthenticated: the auth reference resolves to the empty string, which
    // matches nothing rather than everything.
    let anonymous = AuthContext::default();
    assert_eq!(
        matching(&client, &context, "author = @request.auth.id", &anonymous).await,
        Vec::<String>::new()
    );

    test.cleanup().await;
}

#[tokio::test]
async fn user_values_cannot_change_the_query() {
    let Some(test) = migrated_test_db("injection_is_data").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let auth = AuthContext::default();

    // Every row, for reference.
    let all = matching(&client, &context, "title != null", &auth).await;
    assert_eq!(all.len(), 3);

    // Classic injection payloads must match nothing, and must not error.
    for payload in [
        "title = \"' or 1=1 --\"",
        "title = \"x' or '1'='1\"",
        "title ~ \"' or 1=1 --\"",
        "title = \"'; drop table \\\"posts\\\"; --\"",
    ] {
        let result = matching(&client, &context, payload, &auth).await;
        assert!(
            result.is_empty(),
            "payload {payload:?} matched rows: {result:?}"
        );
    }

    // The same payload negated matches *every* row, because it is compared as a
    // literal string — the clearest proof the value never became syntax.
    assert_eq!(
        matching(&client, &context, "title != \"' or 1=1 --\"", &auth).await,
        vec!["p1", "p2", "p3"]
    );

    // The table is still there and intact.
    let count: i64 = client
        .query_one("select count(*) from \"posts\"", &[])
        .await
        .expect("table survived")
        .get(0);
    assert_eq!(count, 3);

    // An unknown field is refused at compile time, not passed to Postgres.
    let expr = filter::parse("nope = 1").expect("parses");
    let err = compile::compile(&expr, &context, &auth).expect_err("unknown field");
    assert!(err.message().contains("unknown field"), "{err}");

    // ... and a field name that looks like an injection is also refused.
    let expr = filter::parse("title\"; drop table \\\"posts\\\"; -- = 1");
    if let Ok(expr) = expr {
        assert!(
            compile::compile(&expr, &context, &auth).is_err(),
            "field name should have been rejected"
        );
    }

    test.cleanup().await;
}

#[tokio::test]
async fn sort_determines_row_order() {
    let Some(test) = migrated_test_db("sort_order").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;

    let spec = sort::parse("-views").expect("sort");
    let order_by = sort::compile(&spec, &context).expect("compile sort");
    assert_eq!(order_by, "\"views\" desc");
    let ids = select_ids(&client, "posts", "true", &[], &order_by).await;
    assert_eq!(ids, vec!["p2", "p1", "p3"]);

    let spec = sort::parse("views").expect("sort");
    let order_by = sort::compile(&spec, &context).expect("compile sort");
    let ids = select_ids(&client, "posts", "true", &[], &order_by).await;
    assert_eq!(ids, vec!["p3", "p1", "p2"]);

    // Sorting through a relation is refused rather than silently ignored.
    let spec = sort::parse("-author.name").expect("parses");
    assert!(sort::compile(&spec, &context).is_err());

    test.cleanup().await;
}

#[tokio::test]
async fn filters_combine_with_sort_and_stay_parameterized() {
    let Some(test) = migrated_test_db("filter_and_sort").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let expr = filter::parse("published = true").expect("parse");
    let compiled = compile::compile(&expr, &context, &AuthContext::default()).expect("compile");
    let spec = sort::parse("-views").expect("sort");
    let order_by = sort::compile(&spec, &context).expect("compile sort");

    let ids = select_ids(&client, "posts", &compiled.sql, &compiled.params, &order_by).await;
    assert_eq!(ids, vec!["p1", "p3"]);

    test.cleanup().await;
}
