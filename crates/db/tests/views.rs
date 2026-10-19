//! View collections: a read-only collection over a Postgres `VIEW`.
//!
//! Requires a Postgres; see `tests/support/mod.rs`.

mod support;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};
use serde_json::json;

use support::migrated_test_db;
use support::TestDb;

fn base_collection(name: &str, fields: Vec<CreateField>) -> CreateCollection {
    CreateCollection {
        name: name.into(),
        collection_type: CollectionType::Base,
        fields,
        rules: json!({}),
        options: json!({}),
        adopt_existing_table: false,
    }
}

fn text_field(name: &str, required: bool) -> CreateField {
    CreateField {
        name: name.into(),
        field_type: FieldType::Text,
        required,
        unique: false,
        options: json!({}),
    }
}

/// A `posts` table with one row, ready to be read through a view.
async fn posts(test: &TestDb) {
    let mut client = test.client().await;
    collections::create(
        &mut client,
        base_collection("posts", vec![text_field("title", true)]),
    )
    .await
    .expect("create posts");
    client
        .batch_execute("insert into posts (id, title) values ('p1', 'Hello'), ('p2', 'World')")
        .await
        .expect("seed");
}

fn view(name: &str, options: serde_json::Value) -> CreateCollection {
    CreateCollection {
        name: name.into(),
        collection_type: CollectionType::View,
        fields: Vec::new(),
        rules: json!({}),
        options,
        adopt_existing_table: false,
    }
}

#[tokio::test]
async fn a_view_collection_is_created_from_sql_and_reads_its_source() {
    let Some(test) = migrated_test_db("view_create").await else {
        return;
    };
    posts(&test).await;

    let mut client = test.client().await;
    let created = collections::create(
        &mut client,
        view(
            "post_titles",
            json!({ "view_query": "select id, title from posts" }),
        ),
    )
    .await
    .expect("create view");

    assert_eq!(created.collection_type, CollectionType::View);
    // The columns are the query's, derived from the catalog.
    let names: Vec<&str> = created
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(names, vec!["id", "title"]);
    // `id` is a text column, `title` text too.
    assert_eq!(
        created
            .fields
            .iter()
            .find(|field| field.name == "id")
            .map(|field| field.column_type().unwrap()),
        Some("text")
    );

    // A definition reload derives the same fields: they are not stored in `_fields`.
    let listed = collections::find(&client, "post_titles")
        .await
        .expect("find")
        .expect("a view");
    assert_eq!(listed.fields.len(), 2);

    // The view reads its source.
    let rows = client
        .query("select title from post_titles order by id", &[])
        .await
        .expect("select from the view");
    let titles: Vec<String> = rows.iter().map(|row| row.get("title")).collect();
    assert_eq!(titles, vec!["Hello", "World"]);

    // A new source row appears without touching the view.
    client
        .batch_execute("insert into posts (id, title) values ('p3', 'Third')")
        .await
        .expect("insert");
    let count: i64 = client
        .query_one("select count(*) from post_titles", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, 3);

    test.cleanup().await;
}

#[tokio::test]
async fn a_view_builder_spec_is_rendered_and_stored_as_sql() {
    let Some(test) = migrated_test_db("view_builder").await else {
        return;
    };
    posts(&test).await;

    let mut client = test.client().await;
    let created = collections::create(
        &mut client,
        view(
            "titles",
            json!({
                "view_builder": {
                    "source": "posts",
                    "fields": [{ "source": "posts", "name": "title" }],
                    "sort": "title"
                }
            }),
        ),
    )
    .await
    .expect("create from builder");

    let query = created.options["view_query"]
        .as_str()
        .expect("generated sql");
    assert!(query.to_uppercase().contains("FROM"), "{query}");
    assert!(query.contains("\"posts\""), "{query}");
    assert_eq!(created.options["view_origin"], json!("builder"));
    // The builder spec is kept, so the Studio can edit it again.
    assert!(created.options.get("view_builder").is_some());

    test.cleanup().await;
}

#[tokio::test]
async fn a_view_needs_an_id_column_and_a_single_select() {
    let Some(test) = migrated_test_db("view_validation").await else {
        return;
    };
    posts(&test).await;

    let mut client = test.client().await;

    // No `id`: the records API addresses records by id.
    let err = collections::create(
        &mut client,
        view("no_id", json!({ "view_query": "select title from posts" })),
    )
    .await
    .expect_err("a view without id");
    assert!(err.message().contains("`id`"), "{err}");

    // A `;` would end the CREATE VIEW and let the rest run as another statement.
    let err = collections::create(
        &mut client,
        view(
            "second_statement",
            json!({ "view_query": "select id from posts; drop table posts" }),
        ),
    )
    .await
    .expect_err("a second statement");
    assert!(err.message().contains("single SELECT"), "{err}");

    // Neither a query nor a builder.
    let err = collections::create(&mut client, view("empty", json!({})))
        .await
        .expect_err("no definition");
    assert!(err.message().contains("viewQuery"), "{err}");

    // A failed definition leaves nothing behind.
    let exists: bool = client
        .query_one("select to_regclass('no_id') is not null", &[])
        .await
        .expect("check")
        .get(0);
    assert!(!exists, "a refused view must not leave a relation behind");

    test.cleanup().await;
}

#[tokio::test]
async fn the_query_can_be_replaced_and_the_view_dropped() {
    let Some(test) = migrated_test_db("view_update").await else {
        return;
    };
    posts(&test).await;

    let mut client = test.client().await;
    collections::create(
        &mut client,
        view(
            "post_titles",
            json!({ "view_query": "select id, title from posts" }),
        ),
    )
    .await
    .expect("create");

    // A compatible replacement (same columns) is accepted.
    let updated = collections::update(
        &mut client,
        "post_titles",
        collections::UpdateCollection {
            rules: None,
            options: Some(json!({ "view_query": "select id, upper(title) as title from posts" })),
            fields: None,
            delete_missing_fields: false,
        },
    )
    .await
    .expect("replace the query");
    let titles: Vec<String> = client
        .query("select title from post_titles order by id", &[])
        .await
        .expect("select")
        .iter()
        .map(|row| row.get("title"))
        .collect();
    assert_eq!(titles, vec!["HELLO", "WORLD"]);
    assert_eq!(updated.fields.len(), 2);

    // A replacement that drops a column is refused by Postgres, and says why.
    let err = collections::update(
        &mut client,
        "post_titles",
        collections::UpdateCollection {
            rules: None,
            options: Some(json!({ "view_query": "select id from posts" })),
            fields: None,
            delete_missing_fields: false,
        },
    )
    .await
    .expect_err("an incompatible replacement");
    assert!(err.message().contains("replace"), "{err}");

    // Deleting the collection drops the view, not a table.
    collections::delete(&mut client, "post_titles", true)
        .await
        .expect("delete");
    let exists: bool = client
        .query_one("select to_regclass('post_titles') is not null", &[])
        .await
        .expect("check")
        .get(0);
    assert!(!exists, "the view should be gone");
    // The source table is untouched.
    let source: bool = client
        .query_one("select to_regclass('posts') is not null", &[])
        .await
        .expect("check")
        .get(0);
    assert!(source, "the source table must survive");

    test.cleanup().await;
}

#[tokio::test]
async fn a_view_is_read_only_and_cannot_take_fields_or_indexes() {
    let Some(test) = migrated_test_db("view_readonly").await else {
        return;
    };
    posts(&test).await;

    let mut client = test.client().await;
    collections::create(
        &mut client,
        view(
            "post_titles",
            json!({ "view_query": "select id, title from posts" }),
        ),
    )
    .await
    .expect("create");

    // It is a real view (a simple one is auto-updatable in Postgres, so `INSERT`
    // through it is the database's business); the API is what refuses writes, because
    // a view collection is a read-only collection. Assert the relation kind here.
    let kind: String = client
        .query_one(
            "select relkind::text from pg_class where oid = to_regclass('post_titles')",
            &[],
        )
        .await
        .expect("relation kind")
        .get(0);
    assert_eq!(kind, "v", "a view collection must be a view, not a table");

    // Adding a field is refused (a field is a column, and columns come from the query).
    let err = collections::add_field(&mut client, "post_titles", text_field("extra", false))
        .await
        .expect_err("no fields on a view");
    assert!(err.message().to_lowercase().contains("view"), "{err}");

    // A view cannot be indexed.
    let err = collections::create(
        &mut client,
        view(
            "indexed",
            json!({
                "view_query": "select id, title from posts",
                "indexes": ["title"]
            }),
        ),
    )
    .await
    .expect_err("no indexes on a view");
    assert!(err.message().contains("index"), "{err}");

    // And an update that names different fields is refused: the query is the truth.
    let err = collections::update(
        &mut client,
        "post_titles",
        collections::UpdateCollection {
            rules: None,
            options: None,
            fields: Some(vec![text_field("other", false)]),
            delete_missing_fields: false,
        },
    )
    .await
    .expect_err("fields on a view");
    assert!(err.message().contains("query"), "{err}");

    test.cleanup().await;
}
