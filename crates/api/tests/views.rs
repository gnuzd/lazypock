//! The view-builder previews: `dry-run-view` and `preview-view-builder`.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;

use serde_json::{json, Value};

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// Post a preview body and return the reply.
async fn post_preview(app: &TestApp, token: &str, uri: &str, body: Value) -> support::Reply {
    call_authed(&router(app).await, Method::POST, uri, Some(body), token).await
}

/// The message a refusal carries.
fn message(reply: &support::Reply) -> String {
    reply.body.clone().expect("an error body")["message"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn a_dry_run_describes_the_columns_and_samples_rows() {
    let Some(app) = test_app("views_dry_run").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = post_preview(
        &app,
        &token,
        "/api/collections/meta/dry-run-view",
        json!({ "query": "select 1 as the_answer, 'two' as the_text" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let body = reply.body.expect("a preview");
    assert_eq!(
        body["fields"],
        json!([
            { "name": "the_answer", "type": "number" },
            { "name": "the_text", "type": "text" },
        ])
    );
    // The sample is objects, which is what the editor renders.
    assert_eq!(
        body["sample"],
        json!([{ "the_answer": 1, "the_text": "two" }])
    );

    app.cleanup().await;
}

/// A preview must not be a way to make something happen.
#[tokio::test]
async fn a_dry_run_refuses_a_statement_that_writes() {
    let Some(app) = test_app("views_dry_run_write").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = post_preview(
        &app,
        &token,
        "/api/collections/meta/dry-run-view",
        json!({ "query": "delete from _superusers" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    let message = message(&reply);
    // The prefix is what the Studio strips to show the reason.
    assert!(
        message.starts_with("Invalid view query. Raw error:"),
        "the message must keep the prefix, got {message:?}"
    );
    assert!(message.contains("read-only"), "{message:?}");

    app.cleanup().await;
}

#[tokio::test]
async fn a_dry_run_reports_the_database_reason() {
    let Some(app) = test_app("views_dry_run_bad").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = post_preview(
        &app,
        &token,
        "/api/collections/meta/dry-run-view",
        json!({ "query": "select * from definitely_not_a_table" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    let message = message(&reply);
    assert!(
        message.starts_with("Invalid view query. Raw error:"),
        "{message:?}"
    );
    assert!(
        message.contains("definitely_not_a_table"),
        "the reason should name what is wrong, got {message:?}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn the_builder_previews_the_sql_it_would_create() {
    let Some(app) = test_app("views_builder").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // A real collection, because the builder resolves against what is installed.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(json!({
            "name": "vprobe",
            "type": "base",
            "fields": [
                { "name": "title", "type": "text", "required": false, "unique": false, "options": {} }
            ],
            "rules": { "listRule": "", "viewRule": "" },
            "options": {}
        })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    // The API resolves collections from the in-memory cache, which the schema
    // watcher refreshes in a running server. In a test the cache has to be told.
    app.reload_schema().await;

    // One record, so the sample proves the generated SQL actually ran.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections/vprobe/records",
        Some(json!({ "title": "hello" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    let reply = post_preview(
        &app,
        &token,
        "/api/collections/meta/preview-view-builder",
        json!({
            "viewBuilder": {
                "source": "vprobe",
                "fields": [{ "name": "title" }]
            }
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let body = reply.body.expect("a preview");
    // The id is injected, so the view is addressable.
    assert_eq!(
        body["query"],
        json!(
            "SELECT\n    \"vprobe\".\"id\" AS \"id\",\n    \"vprobe\".\"title\" AS \"title\"\nFROM \"vprobe\""
        )
    );
    assert_eq!(
        body["fields"],
        json!([
            { "name": "id", "type": "text" },
            { "name": "title", "type": "text" },
        ])
    );
    let sample = body["sample"].as_array().expect("a sample");
    assert_eq!(sample.len(), 1, "the sample should hold the record");
    assert_eq!(sample[0]["title"], json!("hello"));
    assert_eq!(
        sample[0]["id"].as_str().map(str::len),
        Some(15),
        "the sample should carry the record's id, got {:?}",
        sample[0]["id"]
    );

    app.cleanup().await;
}

#[tokio::test]
async fn the_builder_refuses_a_spec_naming_a_missing_collection() {
    let Some(app) = test_app("views_builder_bad").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = post_preview(
        &app,
        &token,
        "/api/collections/meta/preview-view-builder",
        json!({ "viewBuilder": { "source": "nope", "fields": [] } }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    let message = message(&reply);
    assert!(
        message.starts_with("Invalid view builder spec. Raw error:"),
        "the message must keep the prefix, got {message:?}"
    );
    assert!(message.contains("does not exist"), "{message:?}");

    app.cleanup().await;
}

#[tokio::test]
async fn both_previews_need_a_superuser() {
    let Some(app) = test_app("views_auth").await else {
        return;
    };

    for (uri, body) in [
        (
            "/api/collections/meta/dry-run-view",
            json!({ "query": "select 1" }),
        ),
        (
            "/api/collections/meta/preview-view-builder",
            json!({ "viewBuilder": { "source": "x" } }),
        ),
    ] {
        let reply = call(&router(&app).await, Method::POST, uri, Some(body)).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{uri}");
    }

    app.cleanup().await;
}
