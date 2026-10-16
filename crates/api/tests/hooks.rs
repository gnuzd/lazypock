//! End-to-end tests: a hook file changes what a request stores.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::json;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// A `posts` collection with a `slug` a hook will fill in.
async fn seed(app: &TestApp) {
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: "posts".into(),
            collection_type: CollectionType::Base,
            fields: vec![
                CreateField {
                    name: "title".into(),
                    field_type: FieldType::Text,
                    required: false,
                    unique: false,
                    options: json!({}),
                },
                CreateField {
                    name: "slug".into(),
                    field_type: FieldType::Text,
                    required: true,
                    unique: false,
                    options: json!({}),
                },
            ],
            rules: json!({
                "listRule": "", "viewRule": "", "createRule": "",
                "updateRule": "", "deleteRule": "",
            }),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("create posts");
    app.reload_schema().await;
}

/// Install a hook file and make the worker pick it up.
async fn install(app: &TestApp, source: &str) {
    std::fs::write(app.hooks_directory.join("main.pb.ts"), source).expect("write the hook");
    let report = app.state.hooks.reload().await.expect("reload hooks");
    assert_eq!(
        report.error, None,
        "the hook should have loaded: {report:?}"
    );
}

#[tokio::test]
async fn a_hook_decides_what_a_created_record_stores() {
    let Some(app) = test_app("http_hooks_create").await else {
        return;
    };
    seed(&app).await;

    // A TypeScript hook that derives the slug and ignores whatever was sent.
    install(
        &app,
        r#"
        interface Payload { title: string }
        onRecordBeforeCreate("posts", (e: any) => {
            const payload: Payload = e.record;
            e.record.slug = payload.title.toLowerCase().split(" ").join("-");
        });
        "#,
    )
    .await;

    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections/posts/records",
        Some(json!({ "title": "Hello World", "slug": "ignored" })),
        &token,
    )
    .await;

    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let body = reply.body.expect("a record");
    assert_eq!(
        body["slug"],
        json!("hello-world"),
        "the hook's value is what was stored, not the caller's"
    );

    // ... and the hook's value is what a subsequent read returns.
    let reply = support::get(
        &router(&app).await,
        &format!(
            "/api/collections/posts/records/{}",
            body["id"].as_str().expect("id")
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("a record")["slug"], json!("hello-world"));

    app.cleanup().await;
}

#[tokio::test]
async fn a_hook_that_supplies_a_required_field_makes_the_write_possible() {
    let Some(app) = test_app("http_hooks_required").await else {
        return;
    };
    seed(&app).await;

    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Without a hook the required `slug` is missing, so the write is refused.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections/posts/records",
        Some(json!({ "title": "Hello World" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    // With the hook the same request succeeds, because the hook ran first.
    install(
        &app,
        r#"onRecordBeforeCreate("posts", (e) => { e.record.slug = "supplied"; });"#,
    )
    .await;
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections/posts/records",
        Some(json!({ "title": "Hello World" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    assert_eq!(reply.body.expect("a record")["slug"], json!("supplied"));

    app.cleanup().await;
}

#[tokio::test]
async fn a_hook_cannot_write_something_the_collection_rejects() {
    let Some(app) = test_app("http_hooks_invalid").await else {
        return;
    };
    seed(&app).await;

    // The hook removes the required field, so the write must still be refused.
    install(
        &app,
        r#"onRecordBeforeCreate("posts", (e) => { delete e.record.slug; });"#,
    )
    .await;

    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections/posts/records",
        Some(json!({ "title": "T", "slug": "given" })),
        &token,
    )
    .await;

    assert_eq!(
        reply.status,
        StatusCode::BAD_REQUEST,
        "a hook's output is validated like any other input: {:?}",
        reply.body
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_broken_hook_file_is_reported_and_the_previous_hooks_keep_serving() {
    let Some(app) = test_app("http_hooks_broken").await else {
        return;
    };
    seed(&app).await;
    install(
        &app,
        r#"onRecordBeforeCreate("posts", (e) => { e.record.slug = "good"; });"#,
    )
    .await;

    // A broken edit must not take the working hook down.
    std::fs::write(
        app.hooks_directory.join("main.pb.ts"),
        "onRecordBeforeCreate(",
    )
    .expect("write");
    let report = app.state.hooks.reload().await.expect("reload");
    assert!(report.error.is_some(), "the failure should be reported");

    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections/posts/records",
        Some(json!({ "title": "T" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    assert_eq!(
        reply.body.expect("a record")["slug"],
        json!("good"),
        "the last good hook should still be serving"
    );

    app.cleanup().await;
}

/// Create a record, returning its id.
async fn create_record(app: &TestApp, token: &str) -> String {
    let reply = call_authed(
        &router(app).await,
        Method::POST,
        "/api/collections/posts/records",
        Some(json!({ "title": "T", "slug": "given" })),
        token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body.expect("a record")["id"]
        .as_str()
        .expect("id")
        .to_string()
}

#[tokio::test]
async fn a_before_update_hook_decides_what_is_written() {
    let Some(app) = test_app("http_hooks_update").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let id = create_record(&app, &token).await;

    install(
        &app,
        r#"onRecordBeforeUpdate("posts", (e) => { e.record.slug = "updated-by-hook"; });"#,
    )
    .await;

    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        &format!("/api/collections/posts/records/{id}"),
        Some(json!({ "slug": "ignored" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(
        reply.body.expect("a record")["slug"],
        json!("updated-by-hook"),
        "the hook's value is what was written"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_before_delete_hook_can_refuse_the_delete() {
    let Some(app) = test_app("http_hooks_delete_veto").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let id = create_record(&app, &token).await;

    install(
        &app,
        r#"onRecordBeforeDelete("posts", () => { throw new Error("not so fast"); });"#,
    )
    .await;

    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        &format!("/api/collections/posts/records/{id}"),
        None,
        &token,
    )
    .await;
    assert!(
        reply.status.is_client_error(),
        "a refusing hook should fail the request: {} {:?}",
        reply.status,
        reply.body
    );

    // The record is still there: the refusal happened before the delete.
    let reply = support::get(
        &router(&app).await,
        &format!("/api/collections/posts/records/{id}"),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    // A hook that allows it lets the delete through.
    install(&app, r#"onRecordBeforeDelete("posts", () => {});"#).await;
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        &format!("/api/collections/posts/records/{id}"),
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn an_after_hook_runs_once_the_write_has_committed() {
    let Some(app) = test_app("http_hooks_after_update").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let id = create_record(&app, &token).await;

    // An after-hook that fails is reported, but cannot undo a write that is
    // already durable. That is the documented trade of running it post-commit.
    install(
        &app,
        r#"onRecordAfterUpdate("posts", () => { throw new Error("after"); });"#,
    )
    .await;

    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        &format!("/api/collections/posts/records/{id}"),
        Some(json!({ "slug": "committed" })),
        &token,
    )
    .await;
    assert!(
        reply.status.is_server_error() || reply.status.is_client_error(),
        "the hook failure should be reported: {} {:?}",
        reply.status,
        reply.body
    );

    let reply = support::get(
        &router(&app).await,
        &format!("/api/collections/posts/records/{id}"),
    )
    .await;
    assert_eq!(
        reply.body.expect("a record")["slug"],
        json!("committed"),
        "the write committed before the hook ran"
    );

    app.cleanup().await;
}
