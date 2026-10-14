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
async fn a_collection_can_be_created_read_updated_and_deleted() {
    let Some(app) = test_app("collections_crud").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Nothing yet.
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
    assert_eq!(
        body["items"],
        json!([]),
        "an empty installation lists nothing"
    );
    assert_eq!(body["totalItems"], json!(0));

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
async fn deleting_a_collection_keeps_the_data_unless_asked_to_drop_it() {
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
    sql(&app, "insert into posts (id, title) values ('p1', 'kept')").await;

    // Without `dropData` the definition goes and the rows stay: removing a
    // collection by accident must not destroy data.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/collections/posts",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let client = app.client().await;
    let count: i64 = client
        .query_one("select count(*) from posts", &[])
        .await
        .expect("the table should still exist")
        .get(0);
    assert_eq!(count, 1, "the rows should have survived");

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
