//! View collections over HTTP: create from SQL or the builder, read, refuse writes.
//!
//! The preview endpoints themselves (`dry-run-view`, `preview-view-builder`) are covered
//! in `views.rs`.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::{json, Value};

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

/// A `posts` table (collection) with two rows.
async fn seed(app: &TestApp, token: &str) {
    let reply = call_authed(
        &router(app).await,
        Method::POST,
        "/api/collections",
        Some(json!({
            "name": "posts",
            "type": "base",
            "fields": [{ "name": "title", "type": "text" }],
            "rules": { "listRule": "", "viewRule": "" }
        })),
        token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    sql(
        app,
        "insert into posts (id, title) values ('p1', 'public'), ('p2', 'private')",
    )
    .await;
    app.reload_schema().await;
}

async fn create_view(app: &TestApp, token: &str, body: Value) -> support::Reply {
    let reply = call_authed(
        &router(app).await,
        Method::POST,
        "/api/collections",
        Some(body),
        token,
    )
    .await;
    app.reload_schema().await;
    reply
}

#[tokio::test]
async fn a_view_is_created_from_sql_and_serves_its_rows_read_only() {
    let Some(app) = test_app("views_create").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    seed(&app, &token).await;

    let reply = create_view(
        &app,
        &token,
        json!({
            "name": "public_posts",
            "type": "view",
            "viewQuery": "select id, title from posts where title = 'public'",
            "rules": { "listRule": "", "viewRule": "" }
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let body = reply.body.expect("a view");
    assert_eq!(body["type"], json!("view"));
    // The columns are the query's.
    let fields: Vec<&str> = body["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect();
    assert_eq!(fields, vec!["id", "title"], "{body}");
    assert_eq!(body["viewOrigin"], json!("sql"));
    assert!(body["viewQuery"]
        .as_str()
        .is_some_and(|q| q.contains("posts")));

    // Reading works, and the rule narrows nothing because the query already did.
    let reply = call(
        &router(&app).await,
        Method::GET,
        "/api/collections/public_posts/records",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let list = reply.body.expect("a list");
    assert_eq!(list["totalItems"], json!(1));
    assert_eq!(list["items"][0]["title"], json!("public"));

    // One record by id.
    let reply = call(
        &router(&app).await,
        Method::GET,
        "/api/collections/public_posts/records/p1",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("a record")["title"], json!("public"));

    // A view collection is read-only.
    for (method, uri) in [
        (
            Method::POST,
            "/api/collections/public_posts/records".to_string(),
        ),
        (
            Method::PATCH,
            "/api/collections/public_posts/records/p1".to_string(),
        ),
        (
            Method::DELETE,
            "/api/collections/public_posts/records/p1".to_string(),
        ),
    ] {
        let reply = call_authed(
            &router(&app).await,
            method.clone(),
            &uri,
            Some(json!({ "title": "x" })),
            &token,
        )
        .await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{method} {uri}: {:?}",
            reply.body
        );
        let message = reply.body.expect("an error")["message"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(message.contains("read-only"), "{message}");
    }

    // A batch refuses a view rather than bypassing its rule.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/batch",
        Some(json!({ "requests": [{ "method": "GET", "url": "/api/collections/public_posts/records/p1" }] })),
        &token,
    )
    .await;
    assert_ne!(reply.status, StatusCode::OK, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn a_views_rule_is_enforced_for_a_non_superuser() {
    let Some(app) = test_app("views_rules").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    seed(&app, &token).await;

    // The view exposes both rows; the rule decides who sees which.
    let reply = create_view(
        &app,
        &token,
        json!({
            "name": "post_titles",
            "type": "view",
            "viewQuery": "select id, title from posts",
            "rules": { "listRule": "title = 'public'", "viewRule": "title = 'public'" }
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    // Anonymous: only the public row.
    let reply = call(
        &router(&app).await,
        Method::GET,
        "/api/collections/post_titles/records",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let list = reply.body.expect("a list");
    assert_eq!(list["totalItems"], json!(1), "{list}");
    assert_eq!(list["items"][0]["title"], json!("public"));

    // ... and the private row is a 404, not a leak.
    let reply = call(
        &router(&app).await,
        Method::GET,
        "/api/collections/post_titles/records/p2",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{:?}", reply.body);

    // The superuser sees both, as it does for a table.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections/post_titles/records",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("a list")["totalItems"], json!(2));

    // A view with no rule is superuser-only.
    let reply = create_view(
        &app,
        &token,
        json!({
            "name": "locked",
            "type": "view",
            "viewQuery": "select id, title from posts"
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let reply = call(
        &router(&app).await,
        Method::GET,
        "/api/collections/locked/records",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn a_view_can_be_built_from_the_no_code_spec_and_redefined() {
    let Some(app) = test_app("views_builder").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    seed(&app, &token).await;

    let reply = create_view(
        &app,
        &token,
        json!({
            "name": "titles",
            "type": "view",
            "viewBuilder": {
                "source": "posts",
                "fields": [{ "source": "posts", "name": "title" }],
                "sort": "title"
            },
            "rules": { "listRule": "" }
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let body = reply.body.expect("a view");
    assert_eq!(body["viewOrigin"], json!("builder"));
    assert!(body["viewQuery"]
        .as_str()
        .is_some_and(|query| query.to_uppercase().contains("FROM")));

    // Redefining the query changes what the view returns.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/titles",
        Some(json!({ "viewQuery": "select id, upper(title) as title from posts" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    app.reload_schema().await;

    let reply = call(
        &router(&app).await,
        Method::GET,
        "/api/collections/titles/records?sort=id",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let list = reply.body.expect("a list");
    assert_eq!(list["items"][0]["title"], json!("PUBLIC"));

    // Deleting the collection drops the view and leaves the source alone.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/collections/titles",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    app.reload_schema().await;
    let reply = call(
        &router(&app).await,
        Method::GET,
        "/api/collections/posts/records",
        None,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "the source collection must survive"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_view_cannot_be_subscribed_to() {
    let Some(app) = test_app("views_realtime").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    seed(&app, &token).await;
    let reply = create_view(
        &app,
        &token,
        json!({
            "name": "titles",
            "type": "view",
            "viewQuery": "select id, title from posts",
            "rules": { "listRule": "" }
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    let reply = call(
        &router(&app).await,
        Method::GET,
        "/api/realtime?subscribe=titles",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    app.cleanup().await;
}
