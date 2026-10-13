//! Integration tests for the record write/read path.
//!
//! Requires a Postgres; see `tests/support/mod.rs`.

mod support;

use lazypock_db::collections::{self, Collection, CreateCollection, CreateField};
use lazypock_db::query::compile::FilterContext;
use lazypock_db::query::projection::{ExpandSpec, FieldSelection};
use lazypock_db::records::{self, RecordWriteError};
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

/// Create a `users` and a `posts` collection and return the posts definition.
async fn fixture(client: &mut Client) -> Collection {
    collections::create(
        client,
        collection("users", vec![field("name", FieldType::Text, json!({}))]),
    )
    .await
    .expect("users");
    let users = collections::get(client, "users").await.expect("users");

    let mut title = field("title", FieldType::Text, json!({}));
    title.required = true;
    let mut slug = field("slug", FieldType::Text, json!({}));
    slug.unique = true;

    collections::create(
        client,
        collection(
            "posts",
            vec![
                title,
                slug,
                field(
                    "views",
                    FieldType::Number,
                    json!({"onlyInt": true, "min": 0}),
                ),
                field("published", FieldType::Bool, json!({})),
                field(
                    "tags",
                    FieldType::Select,
                    json!({"values": ["a", "b", "c"], "maxSelect": 3}),
                ),
                field("meta", FieldType::Json, json!({})),
                field(
                    "author",
                    FieldType::Relation,
                    json!({"collectionId": users.id}),
                ),
                field(
                    "published_at",
                    FieldType::Autodate,
                    json!({"onCreate": true}),
                ),
            ],
        ),
    )
    .await
    .expect("posts");

    collections::get(client, "posts").await.expect("posts")
}

async fn context(client: &Client, name: &str) -> FilterContext {
    let all = collections::list(client).await.expect("list");
    let collection = all
        .iter()
        .find(|candidate| candidate.name == name)
        .expect("collection");
    FilterContext::for_collection(collection, &all).expect("context")
}

fn all_fields() -> FieldSelection {
    FieldSelection::all()
}

/// Create a user and return its id.
async fn a_user(client: &Client, collection: &Collection, context: &FilterContext) -> String {
    let record = records::create(client, collection, context, json!({"name": "Alice"}))
        .await
        .expect("create user");
    record["id"].as_str().expect("id").to_string()
}

#[tokio::test]
async fn a_created_record_round_trips_with_real_types() {
    let Some(test) = migrated_test_db("record_create").await else {
        return;
    };
    let mut client = test.client().await;
    let posts = fixture(&mut client).await;
    let users_collection = collections::get(&client, "users").await.expect("users");
    let users_context = context(&client, "users").await;
    let author = a_user(&client, &users_collection, &users_context).await;

    let posts_context = context(&client, "posts").await;
    let record = records::create(
        &client,
        &posts,
        &posts_context,
        json!({
            "title": "Hello",
            "slug": "hello",
            "views": 10,
            "published": true,
            "tags": ["a", "b"],
            "meta": {"seen": 1},
            "author": author,
            "published_at": "2020-01-01T00:00:00Z",
        }),
    )
    .await
    .expect("create");

    // A generated id, plus server-managed timestamps.
    assert_eq!(record["id"].as_str().unwrap().len(), 15);
    assert!(lazypock_core_id_valid(record["id"].as_str().unwrap()));
    assert!(record["created"].is_string());
    assert!(record["updated"].is_string());

    // Values come back with their real JSON types.
    assert_eq!(record["title"], json!("Hello"));
    assert_eq!(record["views"], json!(10));
    assert_eq!(record["published"], json!(true));
    assert_eq!(record["tags"], json!(["a", "b"]));
    assert_eq!(record["meta"], json!({"seen": 1}));
    assert_eq!(record["author"], json!(author));
    // Autodate is server-managed: the client value is ignored.
    assert_eq!(record["published_at"], json!(null));

    // ... and a fetch returns exactly the same document.
    let fetched = records::get_one(
        &client,
        &posts,
        &posts_context,
        record["id"].as_str().unwrap(),
        &all_fields(),
        &ExpandSpec::none(),
    )
    .await
    .expect("get")
    .expect("present");
    assert_eq!(fetched, record);

    test.cleanup().await;
}

fn lazypock_core_id_valid(id: &str) -> bool {
    id.len() == 15 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

#[tokio::test]
async fn a_client_supplied_id_is_honoured_or_rejected() {
    let Some(test) = migrated_test_db("record_client_id").await else {
        return;
    };
    let mut client = test.client().await;
    let posts = fixture(&mut client).await;
    let posts_context = context(&client, "posts").await;

    let record = records::create(
        &client,
        &posts,
        &posts_context,
        json!({"id": "abcDEF123456789", "title": "Custom"}),
    )
    .await
    .expect("create with a custom id");
    assert_eq!(record["id"], json!("abcDEF123456789"));

    let err = records::create(
        &client,
        &posts,
        &posts_context,
        json!({"id": "too-short", "title": "Bad"}),
    )
    .await
    .expect_err("a malformed id must be rejected");
    assert!(matches!(err, RecordWriteError::Other(_)), "{err}");

    test.cleanup().await;
}

#[tokio::test]
async fn validation_failures_are_reported_per_field() {
    let Some(test) = migrated_test_db("record_validation").await else {
        return;
    };
    let mut client = test.client().await;
    let posts = fixture(&mut client).await;
    let posts_context = context(&client, "posts").await;

    let err = records::create(
        &client,
        &posts,
        &posts_context,
        json!({"views": -1, "titel": "typo"}),
    )
    .await
    .expect_err("should fail");

    let RecordWriteError::Validation(errors) = &err else {
        panic!("expected validation errors, got {err}");
    };
    assert_eq!(err.status(), 400);
    assert!(
        errors.fields.contains_key("title"),
        "missing required title"
    );
    assert_eq!(errors.fields["views"].code, "validation_min_number");
    assert_eq!(errors.fields["titel"].code, "validation_unknown_field");

    // The structured data matches PocketBase's error shape.
    let data = err.data();
    assert!(data["title"]["code"].is_string());
    assert!(data["title"]["message"].is_string());

    test.cleanup().await;
}

#[tokio::test]
async fn updates_are_partial_and_report_a_missing_record() {
    let Some(test) = migrated_test_db("record_update").await else {
        return;
    };
    let mut client = test.client().await;
    let posts = fixture(&mut client).await;
    let posts_context = context(&client, "posts").await;

    let created = records::create(
        &client,
        &posts,
        &posts_context,
        json!({"title": "Original", "views": 1, "published": true}),
    )
    .await
    .expect("create");
    let id = created["id"].as_str().expect("id").to_string();

    let updated = records::update(&client, &posts, &posts_context, &id, json!({"views": 42}))
        .await
        .expect("update")
        .expect("present");

    // Only the sent field changed.
    assert_eq!(updated["views"], json!(42));
    assert_eq!(updated["title"], json!("Original"));
    assert_eq!(updated["published"], json!(true));

    // `updated` moved (or at least never went backwards) and `created` did not.
    assert_eq!(updated["created"], created["created"]);
    assert!(
        updated["updated"].as_str().unwrap() >= created["updated"].as_str().unwrap(),
        "updated did not move: {} vs {}",
        updated["updated"],
        created["updated"]
    );

    // A field can be cleared with an explicit null.
    let cleared = records::update(
        &client,
        &posts,
        &posts_context,
        &id,
        json!({"published": null}),
    )
    .await
    .expect("update")
    .expect("present");
    assert_eq!(cleared["published"], json!(null));

    // A required field cannot be cleared.
    let err = records::update(&client, &posts, &posts_context, &id, json!({"title": null}))
        .await
        .expect_err("required field must not be clearable");
    assert!(matches!(err, RecordWriteError::Validation(_)), "{err}");

    // A missing record is `None`, not an error.
    let missing = records::update(
        &client,
        &posts,
        &posts_context,
        "zzzzzzzzzzzzzzz",
        json!({"views": 1}),
    )
    .await
    .expect("update");
    assert!(missing.is_none());

    test.cleanup().await;
}

#[tokio::test]
async fn deletes_report_whether_a_row_was_removed() {
    let Some(test) = migrated_test_db("record_delete").await else {
        return;
    };
    let mut client = test.client().await;
    let posts = fixture(&mut client).await;
    let posts_context = context(&client, "posts").await;

    let created = records::create(&client, &posts, &posts_context, json!({"title": "Gone"}))
        .await
        .expect("create");
    let id = created["id"].as_str().unwrap().to_string();

    assert!(records::delete(&client, &posts, &id).await.expect("delete"));
    assert!(!records::delete(&client, &posts, &id)
        .await
        .expect("delete again"));
    assert!(records::get_one(
        &client,
        &posts,
        &posts_context,
        &id,
        &all_fields(),
        &ExpandSpec::none()
    )
    .await
    .expect("get")
    .is_none());

    test.cleanup().await;
}

#[tokio::test]
async fn database_constraints_surface_as_clean_failures() {
    let Some(test) = migrated_test_db("record_constraints").await else {
        return;
    };
    let mut client = test.client().await;
    let posts = fixture(&mut client).await;
    let users_collection = collections::get(&client, "users").await.expect("users");
    let users_context = context(&client, "users").await;
    let author = a_user(&client, &users_collection, &users_context).await;
    let posts_context = context(&client, "posts").await;

    records::create(
        &client,
        &posts,
        &posts_context,
        json!({"title": "First", "slug": "same", "author": author}),
    )
    .await
    .expect("first");

    // Unique violation -> 409.
    let err = records::create(
        &client,
        &posts,
        &posts_context,
        json!({"title": "Second", "slug": "same"}),
    )
    .await
    .expect_err("duplicate slug");
    assert_eq!(err.status(), 409, "{err}");
    assert!(err.message().contains("unique"), "{err}");

    // Foreign key violation -> 400. The id is well-formed but does not exist.
    let err = records::create(
        &client,
        &posts,
        &posts_context,
        json!({"title": "Third", "slug": "third", "author": "zzzzzzzzzzzzzzz"}),
    )
    .await
    .expect_err("dangling relation");
    assert_eq!(err.status(), 400, "{err}");
    assert!(err.message().contains("does not exist"), "{err}");

    test.cleanup().await;
}

#[tokio::test]
async fn a_fetched_record_can_be_expanded() {
    let Some(test) = migrated_test_db("record_get_expand").await else {
        return;
    };
    let mut client = test.client().await;
    let posts = fixture(&mut client).await;
    let users_collection = collections::get(&client, "users").await.expect("users");
    let users_context = context(&client, "users").await;
    let author = a_user(&client, &users_collection, &users_context).await;
    let posts_context = context(&client, "posts").await;

    let created = records::create(
        &client,
        &posts,
        &posts_context,
        json!({"title": "Expanded", "author": author}),
    )
    .await
    .expect("create");
    let id = created["id"].as_str().unwrap().to_string();

    let selection = FieldSelection::parse(Some("id,author.name"), &posts_context).expect("fields");
    let expand = ExpandSpec::parse(Some("author"), &posts_context).expect("expand");
    let fetched = records::get_one(&client, &posts, &posts_context, &id, &selection, &expand)
        .await
        .expect("get")
        .expect("present");

    assert_eq!(fetched["id"], json!(id));
    assert_eq!(fetched["author"]["name"], json!("Alice"));
    assert!(fetched.get("title").is_none(), "projection was not applied");

    test.cleanup().await;
}

#[tokio::test]
async fn empty_and_non_object_bodies_are_rejected() {
    let Some(test) = migrated_test_db("record_bad_body").await else {
        return;
    };
    let mut client = test.client().await;
    let posts = fixture(&mut client).await;
    let posts_context = context(&client, "posts").await;

    for body in [json!("a string"), json!([1, 2]), json!(5)] {
        let err = records::create(&client, &posts, &posts_context, body)
            .await
            .expect_err("non-object body");
        assert_eq!(err.status(), 400, "{err}");
    }

    test.cleanup().await;
}
