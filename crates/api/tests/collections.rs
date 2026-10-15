//! The collection administration API.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::json;

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

async fn sql(app: &TestApp, statement: &str) {
    let client = app.client().await;
    client
        .batch_execute(statement)
        .await
        .expect("the test statement should apply");
}

/// A minimal collection definition.
fn definition(name: &str) -> serde_json::Value {
    json!({
        "name": name,
        "type": "base",
        "fields": [
            { "name": "title", "type": "text", "required": false, "unique": false, "options": {} }
        ],
        "rules": { "listRule": "", "viewRule": "" },
        "options": {}
    })
}

#[tokio::test]
async fn every_collection_admin_call_needs_a_superuser() {
    let Some(app) = test_app("collections_admin_auth").await else {
        return;
    };

    let cases = [
        (Method::GET, "/api/collections", None),
        (Method::GET, "/api/collections/posts", None),
        (Method::POST, "/api/collections", Some(definition("posts"))),
        (
            Method::PATCH,
            "/api/collections/posts",
            Some(json!({ "rules": {} })),
        ),
        (Method::DELETE, "/api/collections/posts", None),
    ];

    for (method, uri, body) in cases {
        let reply = call(&router(&app).await, method.clone(), uri, body).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{method} {uri} should need a superuser: {:?}",
            reply.body
        );
    }

    app.cleanup().await;
}

#[tokio::test]
async fn system_collections_are_listed_with_a_lock_and_are_not_records() {
    let Some(app) = test_app("collections_system").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.body.expect("a list");
    let superusers = body["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|collection| collection["name"] == json!("_superusers"))
        .expect("_superusers should be listed");
    assert_eq!(
        superusers["system"],
        json!(true),
        "it must be marked system so the Studio locks it"
    );
    assert_eq!(superusers["type"], json!("auth"));
    let field_names: Vec<&str> = superusers["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .map(|field| field["name"].as_str().expect("a name"))
        .collect();
    assert!(field_names.contains(&"email"), "{field_names:?}");
    assert!(field_names.contains(&"password_hash"), "{field_names:?}");

    // A single read resolves it too.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections/_superusers",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("_superusers")["system"], json!(true));

    // But it is **not** a records collection. The schema cache has no
    // `_superusers` (it is not a `_collections` row), so the records API refuses
    // it — which is what keeps superuser rows and their credential hashes off the
    // records endpoints. The `_superusers` table has no row-level security either.
    let reply = support::get(&router(&app).await, "/api/collections/_superusers/records").await;
    assert_eq!(
        reply.status,
        StatusCode::NOT_FOUND,
        "_superusers must not be served as records: {:?}",
        reply.body
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_collection_can_be_created_read_updated_and_deleted() {
    let Some(app) = test_app("collections_crud").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Nothing user-defined yet: the only entry is the synthesised system one.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.body.expect("a list");
    let items = body["items"].as_array().expect("an items array");
    assert_eq!(
        items.len(),
        1,
        "an empty installation lists only _superusers: {body}"
    );
    assert_eq!(items[0]["name"], json!("_superusers"));
    assert_eq!(items[0]["system"], json!(true));
    assert_eq!(body["totalItems"], json!(1));

    // Create.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(definition("posts")),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    // It is listed, with its field.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections",
        None,
        &token,
    )
    .await;
    let listed = reply.body.expect("a list");
    // The list envelope, not a bare array: a client reads `items`.
    let posts = listed["items"]
        .as_array()
        .expect("an items array")
        .iter()
        .find(|collection| collection["name"] == json!("posts"))
        .expect("posts");
    assert_eq!(posts["type"], json!("base"));
    assert_eq!(posts["fields"][0]["name"], json!("title"));

    // A single read returns the same thing.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections/posts",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("a collection")["name"], json!("posts"));

    // Update the rules. The options were not sent, so they must survive.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/posts",
        Some(json!({ "rules": { "listRule": null, "viewRule": "" } })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let updated = reply.body.expect("a collection");
    assert_eq!(updated["rules"]["listRule"], json!(null));
    assert_eq!(updated["rules"]["viewRule"], json!(""));
    assert_eq!(
        updated["fields"][0]["name"],
        json!("title"),
        "fields are not part of a metadata update"
    );

    // Delete the definition, keeping the data.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/collections/posts",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections/posts",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    app.cleanup().await;
}

#[tokio::test]
async fn a_field_can_be_added_and_removed() {
    let Some(app) = test_app("collections_fields").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(definition("posts")),
        &token,
    )
    .await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections/posts/fields",
        Some(json!({
            "name": "views",
            "type": "number",
            "required": false,
            "unique": false,
            "options": {}
        })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    assert_eq!(reply.body.expect("a field")["name"], json!("views"));

    // It is in the definition, and the column exists.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections/posts",
        None,
        &token,
    )
    .await;
    let fields = reply.body.expect("a collection")["fields"].clone();
    let names: Vec<&str> = fields
        .as_array()
        .expect("an array")
        .iter()
        .map(|field| field["name"].as_str().expect("a name"))
        .collect();
    assert!(names.contains(&"views"), "{names:?}");

    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/collections/posts/fields/views",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections/posts",
        None,
        &token,
    )
    .await;
    let fields = reply.body.expect("a collection")["fields"].clone();
    let names: Vec<&str> = fields
        .as_array()
        .expect("an array")
        .iter()
        .map(|field| field["name"].as_str().expect("a name"))
        .collect();
    assert!(!names.contains(&"views"), "{names:?}");

    app.cleanup().await;
}

#[tokio::test]
async fn a_missing_collection_is_a_404_rather_than_a_surprise() {
    let Some(app) = test_app("collections_missing").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    for (method, uri, body) in [
        (Method::GET, "/api/collections/nope", None),
        (
            Method::PATCH,
            "/api/collections/nope",
            Some(json!({ "rules": {} })),
        ),
        (Method::DELETE, "/api/collections/nope", None),
        (
            Method::POST,
            "/api/collections/nope/fields",
            Some(json!({ "name": "x", "type": "text", "options": {} })),
        ),
    ] {
        let reply = call_authed(&router(&app).await, method.clone(), uri, body, &token).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{method} {uri}");
    }

    app.cleanup().await;
}

#[tokio::test]
async fn deleting_a_collection_drops_its_table_by_default() {
    let Some(app) = test_app("collections_drop_data").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(definition("posts")),
        &token,
    )
    .await;
    sql(&app, "insert into posts (id, title) values ('p1', 'gone')").await;

    // With no `dropData`, the table goes — matching `main` and PocketBase. Before
    // this the table was kept, and re-creating the same name then failed.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/collections/posts",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let exists: bool = app
        .client()
        .await
        .query_one("select to_regclass('posts') is not null", &[])
        .await
        .expect("check")
        .get(0);
    assert!(!exists, "deleting a collection must drop its table");

    // And the name can be used again, which is what a user expects after a delete.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(definition("posts")),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn deleting_a_collection_can_keep_the_data_when_asked() {
    let Some(app) = test_app("collections_keep_data").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(definition("posts")),
        &token,
    )
    .await;
    sql(&app, "insert into posts (id, title) values ('p1', 'kept')").await;

    // `dropData=false` is the explicit opt-out: the definition goes, the rows stay,
    // so an intentional metadata-only delete does not destroy data.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/collections/posts?dropData=false",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let count: i64 = app
        .client()
        .await
        .query_one("select count(*) from posts", &[])
        .await
        .expect("the table should still exist")
        .get(0);
    assert_eq!(count, 1, "the rows should have survived");

    app.cleanup().await;
}

#[tokio::test]
async fn flat_rule_keys_from_the_studio_are_stored_and_enforced() {
    let Some(app) = test_app("collections_flat_rules").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // The Studio sends rules as siblings of `name`/`fields`, not nested under
    // `rules`. These used to be silently dropped, so every collection stayed
    // default-deny and a public rule did nothing.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(json!({
            "name": "posts",
            "type": "base",
            "fields": [
                { "name": "title", "type": "text", "required": false, "unique": false }
            ],
            "listRule": "",
            "createRule": "",
            "indexes": []
        })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let created = reply.body.expect("a collection");
    assert_eq!(created["rules"]["listRule"], json!(""));

    // The records API resolves through the schema cache, which a test refreshes
    // explicitly rather than waiting on the NOTIFY watcher.
    app.reload_schema().await;

    // A public create rule really is public: an anonymous write succeeds.
    let reply = support::post(
        &router(&app).await,
        "/api/collections/posts/records",
        json!({ "title": "anonymous" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    // And a rule changed to a filter is enforced too.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/posts",
        Some(json!({ "listRule": "title = 'nope'" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(
        reply.body.as_ref().expect("a collection")["rules"]["listRule"],
        json!("title = 'nope'"),
        "the flat rule must be stored"
    );
    let reply = support::get(&router(&app).await, "/api/collections/posts/records").await;
    let body = reply.body.expect("a list");
    assert_eq!(
        body["totalItems"],
        json!(0),
        "the filter rule must apply: {body}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn field_order_round_trips_as_sort_order() {
    let Some(app) = test_app("collections_field_order").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(json!({
            "name": "posts",
            "type": "base",
            "fields": [
                { "name": "title", "type": "text" },
                { "name": "views", "type": "number" }
            ],
            "rules": { "listRule": "" }
        })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let created = reply.body.expect("a collection");
    assert_eq!(created["fields"][0]["sort_order"], json!(0));
    assert_eq!(created["fields"][1]["sort_order"], json!(1));

    // Send them back in the opposite order, the way a drag does.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/posts",
        Some(json!({
            "fields": [
                { "name": "views", "type": "number" },
                { "name": "title", "type": "text" }
            ]
        })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let updated = reply.body.expect("a collection");
    assert_eq!(updated["fields"][0]["name"], json!("views"));
    assert_eq!(updated["fields"][0]["sort_order"], json!(0));
    assert_eq!(updated["fields"][1]["name"], json!("title"));
    assert_eq!(updated["fields"][1]["sort_order"], json!(1));

    app.cleanup().await;
}

#[tokio::test]
async fn an_unsupported_index_list_is_refused_rather_than_ignored() {
    let Some(app) = test_app("collections_indexes").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(json!({
            "name": "posts",
            "type": "base",
            "fields": [{ "name": "title", "type": "text" }],
            "indexes": ["CREATE INDEX idx_title ON posts (title)"],
            "rules": {}
        })),
        &token,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::NOT_IMPLEMENTED,
        "a custom index must fail loud, not be stored without it: {:?}",
        reply.body
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_reflected_table_shows_up_in_the_api_which_is_what_studio_needs() {
    let Some(app) = test_app("collections_reflected").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // A table lazypock did not create, as an existing database would have.
    sql(
        &app,
        "create table customers (
            id uuid primary key default gen_random_uuid(),
            email text not null unique,
            joined_at timestamptz not null default now()
         )",
    )
    .await;

    let client = app.client().await;
    lazypock_db::reflect::reflect(&client)
        .await
        .expect("reflect");
    app.reload_schema().await;

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let listed = reply.body.expect("a list");
    let customers = listed["items"]
        .as_array()
        .expect("an items array")
        .iter()
        .find(|collection| collection["name"] == json!("customers"))
        .expect("the existing table should be listed as a collection");

    let fields = customers["fields"].as_array().expect("fields");
    let names: Vec<&str> = fields
        .iter()
        .map(|field| field["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, vec!["id", "email", "joined_at"]);

    // The columns arrived with their types.
    let email = fields
        .iter()
        .find(|field| field["name"] == json!("email"))
        .expect("email");
    assert_eq!(email["type"], json!("text"));
    assert_eq!(email["required"], json!(true));
    assert_eq!(email["unique"], json!(true));

    // And it is closed to the public, which is what makes adopting safe.
    assert_eq!(
        customers["rules"],
        json!({}),
        "an adopted collection must not be public"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_patch_applies_the_fields_it_is_given() {
    // The gap that made collection editing useless: the whole definition was sent,
    // a 200 came back, and the fields in it were ignored — so a relation field could
    // not be created or changed through the editor at all.
    let Some(app) = test_app("collections_patch_fields").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // A target for the relation.
    call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(definition("authors")),
        &token,
    )
    .await;
    call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(definition("posts")),
        &token,
    )
    .await;

    // Authenticated: every collection endpoint is superuser-only.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections/authors",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let authors_id = reply.body.expect("authors")["id"]
        .as_str()
        .expect("an id")
        .to_string();

    // Add a relation field through PATCH.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/posts",
        Some(json!({
            "fields": [{
                "name": "author",
                "type": "relation",
                "required": false,
                "unique": false,
                "options": { "collectionId": authors_id, "maxSelect": 1 }
            }]
        })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let fields = reply.body.expect("posts")["fields"].clone();
    let names: Vec<&str> = fields
        .as_array()
        .expect("fields")
        .iter()
        .map(|field| field["name"].as_str().expect("a name"))
        .collect();
    assert!(
        names.contains(&"author"),
        "the field was not added: {names:?}"
    );

    // The column is real, which is what makes the field usable.
    let client = app.client().await;
    let column = client
        .query_opt(
            "select 1 from information_schema.columns \
              where table_name = 'posts' and column_name = 'author' \
                and table_schema = current_schema()",
            &[],
        )
        .await
        .expect("columns");
    assert!(column.is_some(), "the column must exist");

    // Updating an existing field's metadata applies.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/posts",
        Some(json!({
            "fields": [{
                "name": "author",
                "type": "relation",
                "required": true,
                "unique": false,
                "options": { "collectionId": authors_id, "maxSelect": 1 }
            }]
        })),
        &token,
    )
    .await;
    let fields = reply.body.expect("posts")["fields"].clone();
    let author = fields
        .as_array()
        .expect("fields")
        .iter()
        .find(|field| field["name"] == json!("author"))
        .cloned()
        .expect("author");
    assert_eq!(author["required"], json!(true), "metadata was not applied");

    // A field the payload omits is left alone rather than dropped: an editor that
    // sends a partial list must not silently take a column with it.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/posts",
        Some(json!({
            "fields": [{
                "name": "another",
                "type": "text",
                "required": false,
                "unique": false,
                "options": {}
            }]
        })),
        &token,
    )
    .await;
    let fields = reply.body.expect("posts")["fields"].clone();
    let names: Vec<&str> = fields
        .as_array()
        .expect("fields")
        .iter()
        .map(|field| field["name"].as_str().expect("a name"))
        .collect();
    assert!(
        names.contains(&"author") && names.contains(&"title"),
        "nothing may be dropped by omission: {names:?}"
    );
    assert!(names.contains(&"another"), "{names:?}");

    // And a type change is refused by name rather than ignored, because it is a column
    // migration rather than an edit.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/posts",
        Some(json!({
            "fields": [{
                "name": "author",
                "type": "text",
                "required": false,
                "unique": false,
                "options": {}
            }]
        })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let message = reply.body.expect("an error")["message"]
        .as_str()
        .expect("a message")
        .to_string();
    assert!(message.contains("author"), "{message}");
    assert!(message.contains("relation"), "{message}");

    app.cleanup().await;
}

#[tokio::test]
async fn a_camel_case_field_works_end_to_end() {
    // Mixed case is what PocketBase and its clients use (`emailVisibility`), and it
    // was being refused. Allowing it is only safe if every column reference is
    // quoted, so this asserts the whole path rather than just that the name is
    // accepted: create, read, filter and sort.
    let Some(app) = test_app("collections_camel_case").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(json!({
            "name": "profiles",
            "type": "base",
            "fields": [
                { "name": "firstName", "type": "text", "required": false, "unique": false },
                { "name": "lastLoginAt", "type": "text", "required": false, "unique": false }
            ],
            "rules": {
                "listRule": "", "viewRule": "", "createRule": "",
                "updateRule": "", "deleteRule": ""
            },
            "options": {}
        })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let fields = reply.body.expect("a collection")["fields"].clone();
    let names: Vec<&str> = fields
        .as_array()
        .expect("fields")
        .iter()
        .map(|field| field["name"].as_str().expect("a name"))
        .collect();
    assert!(names.contains(&"firstName"), "{names:?}");

    // A collection created over HTTP reaches this node's schema cache through the
    // NOTIFY watcher, which is asynchronous — that is what keeps every node in step,
    // so a script has to wait for it rather than assume it is instant.
    app.reload_schema().await;

    // A record carrying the camelCase columns.
    let reply = support::post(
        &router(&app).await,
        "/api/collections/profiles/records",
        json!({ "firstName": "Ada", "lastLoginAt": "2026-01-01" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    assert_eq!(
        reply.body.expect("a record")["firstName"],
        json!("Ada"),
        "the value must come back under its own name, not a folded one"
    );

    // A second record, so sorting has something to do.
    support::post(
        &router(&app).await,
        "/api/collections/profiles/records",
        json!({ "firstName": "Zoe", "lastLoginAt": "2026-02-01" }),
    )
    .await;

    // Filtering by a camelCase field — the filter compiler has to quote it.
    let reply = support::get(
        &router(&app).await,
        "/api/collections/profiles/records?filter=firstName%3D%22Ada%22",
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let items = reply.body.expect("a list")["items"].clone();
    assert_eq!(
        items.as_array().expect("items").len(),
        1,
        "filtering on a camelCase field must work: {items}"
    );

    // Sorting by it, which quotes the column in the ORDER BY.
    let reply = support::get(
        &router(&app).await,
        "/api/collections/profiles/records?sort=firstName",
    )
    .await;
    let items = reply.body.expect("a list")["items"].clone();
    let first = items.as_array().expect("items").first().expect("a record")["firstName"].clone();
    assert_eq!(first, json!("Ada"), "sorted by the camelCase column");

    // And a name that is genuinely not an identifier is still refused.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(json!({
            "name": "bad_name",
            "type": "base",
            "fields": [{ "name": "has space", "type": "text" }],
            "rules": {},
            "options": {}
        })),
        &token,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::BAD_REQUEST,
        "a name with a space must still be refused: {:?}",
        reply.body
    );

    app.cleanup().await;
}
