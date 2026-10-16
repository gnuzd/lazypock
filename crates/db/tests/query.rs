//! Integration tests for the query plane: the compiled SQL is executed against
//! a real Postgres, so this also proves the generated statements are valid and
//! that user values are treated as data rather than syntax.
//!
//! Requires a Postgres; see `tests/support/mod.rs`.

mod support;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::ddl::quote_ident;
use lazypock_db::query::compile::{self, AuthContext, FilterContext};
use lazypock_db::query::pagination::{Cursor, Page};
use lazypock_db::query::projection::{ExpandSpec, FieldSelection};
use lazypock_db::query::read::{self, BuiltQuery, ReadQuery};
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

// ------------------------------------------------------------- read builder

/// Run a built statement and return the JSON documents it produced.
async fn records(client: &Client, built: &BuiltQuery) -> Vec<serde_json::Value> {
    let bound: Vec<&(dyn ToSql + Sync)> = built
        .params
        .iter()
        .map(|param| param as &(dyn ToSql + Sync))
        .collect();
    client
        .query(&built.sql, &bound)
        .await
        .unwrap_or_else(|err| panic!("read query failed: {err}\n{}", built.sql))
        .iter()
        .map(|row| row.get::<_, serde_json::Value>(0))
        .collect()
}

fn read_query(
    context: &FilterContext,
    fields: &str,
    expand: Option<&str>,
    filter_input: Option<&str>,
    sort_input: &str,
    page: Page,
) -> ReadQuery {
    ReadQuery {
        selection: FieldSelection::parse(Some(fields), context).expect("fields"),
        expand: ExpandSpec::parse(expand, context).expect("expand"),
        filter: filter_input.map(|input| filter::parse(input).expect("filter")),
        sort: sort::parse(sort_input).expect("sort"),
        page,
    }
}

fn first_page() -> Page {
    Page::from_params(None, None, false, None).expect("page")
}

#[tokio::test]
async fn the_read_builder_projects_only_the_requested_fields() {
    let Some(test) = migrated_test_db("read_projection").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let query = read_query(&context, "id,title", None, None, "id", first_page());
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;

    assert_eq!(rows.len(), 3);
    let first = rows[0].as_object().expect("object");
    assert_eq!(first.len(), 2, "only the selected keys: {first:?}");
    assert_eq!(rows[0]["id"], json!("p1"));
    assert_eq!(rows[0]["title"], json!("Hello World"));
    assert!(rows[0].get("views").is_none());

    // The default projection carries every column, with real JSON types.
    let query = read_query(&context, "*", None, None, "id", first_page());
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;
    assert_eq!(rows[0]["views"], json!(10));
    assert_eq!(rows[0]["published"], json!(true));
    assert_eq!(rows[0]["rating"], json!(1.5));
    assert_eq!(rows[0]["tags"], json!(["a", "b"]));

    test.cleanup().await;
}

#[tokio::test]
async fn expanding_a_single_relation_nests_its_document() {
    let Some(test) = migrated_test_db("read_expand_single").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let query = read_query(
        &context,
        "id,author.name",
        Some("author"),
        None,
        "id",
        first_page(),
    );
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;

    // The expansion lives under `expand`; the relation's own key keeps the id —
    // PocketBase's shape, which the SDK and the Studio read.
    assert_eq!(rows[0]["expand"]["author"]["name"], json!("Alice"));
    assert_eq!(rows[1]["expand"]["author"]["name"], json!("Bob"));
    assert!(
        rows[0].get("author").is_none(),
        "`fields` did not select the id"
    );
    // The nested `fields` entry narrows the expanded record to one key.
    assert_eq!(rows[0]["expand"]["author"].as_object().unwrap().len(), 1);

    test.cleanup().await;
}

#[tokio::test]
async fn expanding_a_multi_relation_produces_an_array() {
    let Some(test) = migrated_test_db("read_expand_multi").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let query = read_query(
        &context,
        "id,reviewers.name",
        Some("reviewers"),
        None,
        "id",
        first_page(),
    );
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;

    assert_eq!(rows[0]["expand"]["reviewers"], json!([{ "name": "Bob" }]));
    assert_eq!(rows[1]["expand"]["reviewers"].as_array().unwrap().len(), 2);
    // No related rows is an empty array, never null.
    assert_eq!(rows[2]["expand"]["reviewers"], json!([]));

    test.cleanup().await;
}

#[tokio::test]
async fn expanding_everything_covers_the_relations_one_level_deep() {
    let Some(test) = migrated_test_db("read_expand_all").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let query = read_query(&context, "id", Some("*"), None, "id", first_page());
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;

    assert_eq!(rows[0]["expand"]["author"]["name"], json!("Alice"));
    assert!(rows[0]["expand"]["reviewers"].is_array());
    // Only the selected top-level key plus `expand`.
    assert!(rows[0].get("title").is_none());

    test.cleanup().await;
}

#[tokio::test]
async fn the_read_builder_combines_filter_sort_and_projection() {
    let Some(test) = migrated_test_db("read_filter_sort").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let query = read_query(
        &context,
        "id,views",
        None,
        Some("published = true && views > 5"),
        "-views",
        first_page(),
    );
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], json!("p1"));

    test.cleanup().await;
}

#[tokio::test]
async fn count_matches_the_filtered_rows() {
    let Some(test) = migrated_test_db("read_count").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;

    for (input, expected) in [
        (None, 3_i64),
        (Some("published = true"), 2),
        (Some("views > 100"), 0),
    ] {
        let expr = input.map(|raw| filter::parse(raw).expect("filter"));
        let built =
            read::build_count(expr.as_ref(), &context, &AuthContext::default()).expect("build");
        let bound: Vec<&(dyn ToSql + Sync)> = built
            .params
            .iter()
            .map(|param| param as &(dyn ToSql + Sync))
            .collect();
        let count: i64 = client
            .query_one(&built.sql, &bound)
            .await
            .expect("count")
            .get(0);
        assert_eq!(count, expected, "filter {input:?}");
    }

    test.cleanup().await;
}

#[tokio::test]
async fn offset_paging_walks_the_result_set() {
    let Some(test) = migrated_test_db("read_offset_paging").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;

    let page = Page::from_params(Some(1), Some(2), false, None).expect("page 1");
    let query = read_query(&context, "id", None, None, "id", page);
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], json!("p1"));
    assert_eq!(rows[1]["id"], json!("p2"));

    let page = Page::from_params(Some(2), Some(2), false, None).expect("page 2");
    let query = read_query(&context, "id", None, None, "id", page);
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], json!("p3"));

    test.cleanup().await;
}

#[tokio::test]
async fn keyset_paging_continues_without_repeating_or_skipping() {
    let Some(test) = migrated_test_db("read_keyset_paging").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    // Distinct, known timestamps so the ordering is deterministic.
    client
        .batch_execute(
            r#"
            update "posts" set "created" = '2026-01-01T00:00:00Z'::timestamptz where "id" = 'p1';
            update "posts" set "created" = '2026-01-02T00:00:00Z'::timestamptz where "id" = 'p2';
            update "posts" set "created" = '2026-01-03T00:00:00Z'::timestamptz where "id" = 'p3';
            "#,
        )
        .await
        .expect("set timestamps");

    let context = filter_context(&client, "posts").await;

    // Newest first: p3, p2, p1.
    let page = Page::from_params(None, Some(2), false, Some("")).expect("first keyset page");
    let query = read_query(&context, "id", None, None, "-created", page);
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;
    let ids: Vec<&serde_json::Value> = rows.iter().map(|row| &row["id"]).collect();
    assert_eq!(ids, vec![&json!("p3"), &json!("p2")]);

    // Build the cursor from the last row of the page, exactly as a client would.
    let last_id = "p2";
    let created: std::time::SystemTime = client
        .query_one(
            "select \"created\" from \"posts\" where \"id\" = $1",
            &[&last_id],
        )
        .await
        .expect("created")
        .get(0);
    let cursor = Cursor::from_time(created, last_id).expect("cursor");

    let page = Page::from_params(None, Some(2), false, Some(&cursor.encode())).expect("keyset");
    let query = read_query(&context, "id", None, None, "-created", page);
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;
    assert_eq!(
        rows.iter().map(|row| row["id"].clone()).collect::<Vec<_>>(),
        vec![json!("p1")],
        "second page must continue, not repeat or skip"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn the_read_builder_stays_parameterized() {
    let Some(test) = migrated_test_db("read_parameterized").await else {
        return;
    };
    let mut client = test.client().await;
    seed(&mut client).await;

    let context = filter_context(&client, "posts").await;
    let query = read_query(
        &context,
        "id",
        None,
        Some("title = \"' or 1=1 --\""),
        "id",
        first_page(),
    );
    let built = read::build_list(&query, &context, &AuthContext::default()).expect("build");
    let rows = records(&client, &built).await;
    assert!(rows.is_empty(), "injection payload matched rows: {rows:?}");

    test.cleanup().await;
}
