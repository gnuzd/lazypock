//! Integration tests for the collection/field model, the DDL engine and the
//! schema cache.
//!
//! Requires a Postgres; see `tests/support/mod.rs`.

mod support;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::schema::SchemaCache;
use lazypock_db::types::{CollectionType, FieldType};
use serde_json::json;

use support::migrated_test_db;

fn text_field(name: &str, required: bool) -> CreateField {
    CreateField {
        name: name.into(),
        field_type: FieldType::Text,
        required,
        unique: false,
        options: json!({}),
    }
}

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

#[tokio::test]
async fn creating_a_collection_creates_a_real_table_and_stores_its_fields() {
    let Some(test) = migrated_test_db("create_collection").await else {
        return;
    };

    let mut client = test.client().await;
    let mut input = base_collection("posts", vec![text_field("title", true)]);
    input.fields[0].unique = true;

    let created = collections::create(&mut client, input)
        .await
        .expect("create");

    assert_eq!(created.name, "posts");
    assert_eq!(created.fields.len(), 1);
    assert!(created.id.len() == 15);

    // The table exists with the system columns and the user field.
    let columns = test.columns("posts").await;
    assert_eq!(columns, vec!["id", "created", "updated", "title"]);

    // Metadata round-trips.
    let fetched = collections::get(&client, "posts").await.expect("get");
    assert_eq!(fetched, created);
    assert_eq!(collections::list(&client).await.expect("list").len(), 1);

    // A unique index was created for the unique field.
    let indexes: Vec<String> = client
        .query(
            "select indexname from pg_indexes where schemaname = current_schema() and tablename = 'posts'",
            &[],
        )
        .await
        .expect("indexes")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert!(
        indexes.iter().any(|name| name == "idx_posts_title"),
        "missing unique index: {indexes:?}"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn field_types_map_to_the_expected_postgres_columns() {
    let Some(test) = migrated_test_db("field_types").await else {
        return;
    };

    let mut client = test.client().await;
    let input = base_collection(
        "mixed",
        vec![
            CreateField {
                name: "views".into(),
                field_type: FieldType::Number,
                required: false,
                unique: false,
                options: json!({"onlyInt": true}),
            },
            CreateField {
                name: "ratio".into(),
                field_type: FieldType::Number,
                required: false,
                unique: false,
                options: json!({}),
            },
            CreateField {
                name: "tags".into(),
                field_type: FieldType::Select,
                required: false,
                unique: false,
                options: json!({"values": ["a", "b"], "maxSelect": 3}),
            },
            CreateField {
                name: "meta".into(),
                field_type: FieldType::Json,
                required: false,
                unique: false,
                options: json!({}),
            },
            CreateField {
                name: "published".into(),
                field_type: FieldType::Bool,
                required: false,
                unique: false,
                options: json!({}),
            },
        ],
    );
    collections::create(&mut client, input)
        .await
        .expect("create");

    assert_eq!(test.column_type("mixed", "views").await.unwrap(), "bigint");
    assert_eq!(
        test.column_type("mixed", "ratio").await.unwrap(),
        "double precision"
    );
    assert_eq!(test.column_type("mixed", "tags").await.unwrap(), "ARRAY");
    assert_eq!(test.column_type("mixed", "meta").await.unwrap(), "jsonb");
    assert_eq!(
        test.column_type("mixed", "published").await.unwrap(),
        "boolean"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn auth_collections_get_the_auth_columns() {
    let Some(test) = migrated_test_db("auth_collection").await else {
        return;
    };

    let mut client = test.client().await;
    let input = CreateCollection {
        name: "users".into(),
        collection_type: CollectionType::Auth,
        fields: vec![],
        rules: json!({}),
        options: json!({}),
        adopt_existing_table: false,
    };
    collections::create(&mut client, input)
        .await
        .expect("create");

    let columns = test.columns("users").await;
    for expected in ["email", "password_hash", "verified", "token_key"] {
        assert!(
            columns.contains(&expected.to_string()),
            "missing {expected}: {columns:?}"
        );
    }

    test.cleanup().await;
}

#[tokio::test]
async fn invalid_names_and_options_are_rejected_before_any_ddl_runs() {
    let Some(test) = migrated_test_db("invalid_input").await else {
        return;
    };

    let mut client = test.client().await;

    let err = collections::create(&mut client, base_collection("Posts", vec![]))
        .await
        .expect_err("uppercase names must be rejected");
    assert!(err.message().contains("lowercase"), "{err}");

    let err = collections::create(&mut client, base_collection("_hidden", vec![]))
        .await
        .expect_err("underscore prefix must be rejected");
    assert!(err.message().contains("reserved"), "{err}");

    let err = collections::create(
        &mut client,
        base_collection("bad_field", vec![text_field("drop table x", false)]),
    )
    .await
    .expect_err("invalid field names must be rejected");
    assert!(err.message().contains("field name"), "{err}");

    let err = collections::create(
        &mut client,
        base_collection(
            "bad_select",
            vec![CreateField {
                name: "status".into(),
                field_type: FieldType::Select,
                required: false,
                unique: false,
                options: json!({"values": ["a"], "maxSelect": 0}),
            }],
        ),
    )
    .await
    .expect_err("maxSelect=0 must be rejected");
    assert!(err.message().contains("maxSelect"), "{err}");

    // Nothing was created.
    assert!(collections::list(&client).await.expect("list").is_empty());
    assert!(test.tables().await.iter().all(|t| t.starts_with('_')));

    test.cleanup().await;
}

#[tokio::test]
async fn duplicate_collection_names_and_taken_tables_conflict() {
    let Some(test) = migrated_test_db("conflicts").await else {
        return;
    };

    let mut client = test.client().await;
    collections::create(&mut client, base_collection("posts", vec![]))
        .await
        .expect("first create");

    let err = collections::create(&mut client, base_collection("posts", vec![]))
        .await
        .expect_err("duplicate name must conflict");
    assert!(err.message().contains("already exists"), "{err}");

    // A table that is not a declared collection must not be adopted silently.
    let other = test.client().await;
    other
        .batch_execute("create table \"rogue\" (\"id\" text)")
        .await
        .expect("rogue table");
    let err = collections::create(&mut client, base_collection("rogue", vec![]))
        .await
        .expect_err("existing table must conflict by default");
    assert!(err.message().contains("already exists"), "{err}");

    // ... and adopting is refused when the table does not match the declaration,
    // so metadata can never misdescribe the real schema.
    let mut adopt = base_collection("rogue", vec![text_field("title", false)]);
    adopt.adopt_existing_table = true;
    let err = collections::create(&mut client, adopt)
        .await
        .expect_err("adopting a mismatched table must fail");
    assert!(err.message().contains("missing the column"), "{err}");

    test.cleanup().await;
}

#[tokio::test]
async fn unique_fields_are_enforced_by_the_database() {
    let Some(test) = migrated_test_db("unique_field").await else {
        return;
    };

    let mut client = test.client().await;
    let mut input = base_collection("posts", vec![text_field("slug", true)]);
    input.fields[0].unique = true;
    collections::create(&mut client, input)
        .await
        .expect("create");

    client
        .execute(
            "insert into \"posts\" (\"id\", \"slug\") values ('p1', 'hello')",
            &[],
        )
        .await
        .expect("first insert");

    let err = client
        .execute(
            "insert into \"posts\" (\"id\", \"slug\") values ('p2', 'hello')",
            &[],
        )
        .await
        .expect_err("duplicate slug must violate the unique index");
    assert!(
        err.as_db_error()
            .map(|db| db.code().code() == "23505")
            .unwrap_or(false),
        "expected a unique violation, got {err}"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn single_relations_are_foreign_keys() {
    let Some(test) = migrated_test_db("relation_fk").await else {
        return;
    };

    let mut client = test.client().await;
    collections::create(&mut client, base_collection("users", vec![]))
        .await
        .expect("users");
    let users = collections::get(&client, "users").await.expect("get users");

    collections::create(
        &mut client,
        base_collection(
            "posts",
            vec![CreateField {
                name: "author".into(),
                field_type: FieldType::Relation,
                required: false,
                unique: false,
                options: json!({"collectionId": users.id}),
            }],
        ),
    )
    .await
    .expect("posts");

    // A valid reference is accepted...
    client
        .execute("insert into \"users\" (\"id\") values ('u1')", &[])
        .await
        .expect("user row");
    client
        .execute(
            "insert into \"posts\" (\"id\", \"author\") values ('p1', 'u1')",
            &[],
        )
        .await
        .expect("valid reference");

    // ... an unknown one is rejected by the database.
    let err = client
        .execute(
            "insert into \"posts\" (\"id\", \"author\") values ('p2', 'nope')",
            &[],
        )
        .await
        .expect_err("dangling reference must violate the foreign key");
    assert!(
        err.as_db_error()
            .map(|db| db.code().code() == "23503")
            .unwrap_or(false),
        "expected a foreign key violation, got {err}"
    );

    // Deleting the target nulls the reference (default `on delete set null`).
    client
        .execute("delete from \"users\" where \"id\" = 'u1'", &[])
        .await
        .expect("delete user");
    let author: Option<String> = client
        .query_one("select \"author\" from \"posts\" where \"id\" = 'p1'", &[])
        .await
        .expect("select")
        .get(0);
    assert_eq!(author, None);

    test.cleanup().await;
}

#[tokio::test]
async fn adding_a_required_field_backfills_existing_rows_then_drops_the_default() {
    let Some(test) = migrated_test_db("add_required_field").await else {
        return;
    };

    let mut client = test.client().await;
    collections::create(&mut client, base_collection("posts", vec![]))
        .await
        .expect("create");
    client
        .execute("insert into \"posts\" (\"id\") values ('p1')", &[])
        .await
        .expect("seed row");

    let field = collections::add_field(&mut client, "posts", text_field("slug", true))
        .await
        .expect("add required field");

    assert!(field.required);
    // The existing row was backfilled rather than the add failing.
    let slug: String = client
        .query_one("select \"slug\" from \"posts\" where \"id\" = 'p1'", &[])
        .await
        .expect("row still present")
        .get(0);
    assert_eq!(slug, "");

    // The column is NOT NULL and no leftover default masks a missing value.
    let nullable: String = client
        .query_one(
            "select is_nullable from information_schema.columns \
             where table_schema = current_schema() and table_name = 'posts' and column_name = 'slug'",
            &[],
        )
        .await
        .expect("column metadata")
        .get(0);
    assert_eq!(nullable, "NO");

    let err = client
        .execute("insert into \"posts\" (\"id\") values ('p2')", &[])
        .await
        .expect_err("the placeholder default must have been dropped");
    assert!(err.as_db_error().is_some());

    test.cleanup().await;
}

#[tokio::test]
async fn adding_a_required_relation_fails_loud_instead_of_inventing_data() {
    let Some(test) = migrated_test_db("required_relation").await else {
        return;
    };

    let mut client = test.client().await;
    collections::create(&mut client, base_collection("users", vec![]))
        .await
        .expect("users");
    let users = collections::get(&client, "users").await.expect("get");
    collections::create(&mut client, base_collection("posts", vec![]))
        .await
        .expect("posts");

    let err = collections::add_field(
        &mut client,
        "posts",
        CreateField {
            name: "author".into(),
            field_type: FieldType::Relation,
            required: true,
            unique: false,
            options: json!({"collectionId": users.id}),
        },
    )
    .await
    .expect_err("a required relation has no safe placeholder");
    assert!(err.message().contains("no safe placeholder"), "{err}");

    test.cleanup().await;
}

#[tokio::test]
async fn fields_can_be_added_and_removed_and_their_columns_follow() {
    let Some(test) = migrated_test_db("add_remove_field").await else {
        return;
    };

    let mut client = test.client().await;
    collections::create(&mut client, base_collection("posts", vec![]))
        .await
        .expect("create");

    collections::add_field(&mut client, "posts", text_field("subtitle", false))
        .await
        .expect("add");
    assert!(test
        .columns("posts")
        .await
        .contains(&"subtitle".to_string()));

    let err = collections::add_field(&mut client, "posts", text_field("subtitle", false))
        .await
        .expect_err("duplicate field must conflict");
    assert!(err.message().contains("already has a field"), "{err}");

    let removed = collections::delete_field(&mut client, "posts", "subtitle")
        .await
        .expect("delete field");
    assert_eq!(removed.name, "subtitle");
    assert!(!test
        .columns("posts")
        .await
        .contains(&"subtitle".to_string()));

    let err = collections::delete_field(&mut client, "posts", "subtitle")
        .await
        .expect_err("removing a missing field must fail");
    assert!(err.message().contains("no field named"), "{err}");

    test.cleanup().await;
}

#[tokio::test]
async fn deleting_a_collection_drops_the_table_only_when_asked() {
    let Some(test) = migrated_test_db("delete_collection").await else {
        return;
    };

    let mut client = test.client().await;
    collections::create(&mut client, base_collection("posts", vec![]))
        .await
        .expect("posts");

    // Keep the data: metadata goes, the table stays.
    collections::delete(&mut client, "posts", false)
        .await
        .expect("delete metadata");
    assert!(collections::get(&client, "posts").await.is_err());
    assert!(test.tables().await.contains(&"posts".to_string()));

    // Re-declaring it with adoption attaches to the kept table: the recovery
    // path after a metadata-only delete. It must be explicit.
    let mut adopt = base_collection("posts", vec![]);
    adopt.adopt_existing_table = true;
    collections::create(&mut client, adopt)
        .await
        .expect("adopt over the kept table");

    // Drop the data too.
    collections::delete(&mut client, "posts", true)
        .await
        .expect("delete with data");
    assert!(!test.tables().await.contains(&"posts".to_string()));

    test.cleanup().await;
}

#[tokio::test]
async fn the_schema_cache_reflects_changes_after_a_reload() {
    let Some(test) = migrated_test_db("schema_cache").await else {
        return;
    };

    let mut client = test.client().await;
    let cache = SchemaCache::new();
    assert!(cache.snapshot().is_empty());

    collections::create(&mut client, base_collection("posts", vec![]))
        .await
        .expect("create");

    cache.reload(&client).await.expect("reload");
    assert_eq!(cache.snapshot().get("posts").unwrap().name, "posts");

    collections::delete(&mut client, "posts", true)
        .await
        .expect("delete");
    cache.reload(&client).await.expect("reload");
    assert!(cache.snapshot().get("posts").is_none());

    test.cleanup().await;
}

#[tokio::test]
async fn every_mutation_notifies_the_schema_changed_channel() {
    let Some(test) = migrated_test_db("schema_notify").await else {
        return;
    };

    // Listen on a dedicated connection, as another node would. Channel names are
    // database-global (not schema-scoped), so no search_path is needed here.
    let mut notifications =
        support::spawn_notification_listener(&test.admin_url, collections::SCHEMA_CHANGED_CHANNEL);

    let mut client = test.client().await;
    collections::create(&mut client, base_collection("posts", vec![]))
        .await
        .expect("create");

    let notification =
        tokio::time::timeout(std::time::Duration::from_secs(10), notifications.recv())
            .await
            .expect("expected a schema-changed notification")
            .expect("the listener is still alive");

    assert_eq!(notification.channel(), collections::SCHEMA_CHANGED_CHANNEL);

    test.cleanup().await;
}

#[tokio::test]
async fn a_relation_may_name_its_target_instead_of_identifying_it() {
    // Studio sends `"options": { "collection": "users" }` — the target's *name* —
    // where PocketBase stores `collectionId`. Three places read that option: the
    // validator, the foreign-key builder and the filter compiler. Two were taught to
    // accept a name; the third was not, and it was the one the request actually
    // reached, so a relation field was accepted by validation, given a foreign key,
    // and then refused with "has no collectionId". Hence one accessor, used by all
    // three, and this test.
    use lazypock_db::collections::{self, CreateCollection, CreateField};
    use lazypock_db::types::{CollectionType, FieldType};
    use serde_json::json;

    let Some(db) = support::migrated_test_db("relation_target_by_name").await else {
        return;
    };
    let mut client = db.client().await;

    for name in ["authors", "posts"] {
        let fields = if name == "posts" {
            vec![CreateField {
                name: "author".into(),
                field_type: FieldType::Relation,
                required: false,
                unique: false,
                // The name, not an id — Studio's shape.
                options: json!({ "collection": "authors", "maxSelect": 1 }),
            }]
        } else {
            Vec::new()
        };

        collections::create(
            &mut client,
            CreateCollection {
                name: name.into(),
                collection_type: CollectionType::Base,
                fields,
                rules: json!({}),
                options: json!({}),
                adopt_existing_table: false,
            },
        )
        .await
        .unwrap_or_else(|err| panic!("create {name}: {err}"));
    }

    // The foreign key exists, which is the DDL having resolved the name.
    let constraint: i64 = client
        .query_one(
            "select count(*) from information_schema.table_constraints \
              where table_name = 'posts' and constraint_type = 'FOREIGN KEY' \
                and table_schema = current_schema()",
            &[],
        )
        .await
        .expect("constraints")
        .get(0);
    assert_eq!(
        constraint, 1,
        "a relation naming its target must still get a foreign key"
    );

    // And the filter compiler resolves it too — the reader that was missing.
    let all = collections::list(&client).await.expect("list");
    let posts = all
        .iter()
        .find(|collection| collection.name == "posts")
        .cloned()
        .expect("posts");
    lazypock_db::query::compile::FilterContext::for_collection(&posts, &all)
        .expect("the filter compiler must resolve the relation by name");

    db.cleanup().await;
}
