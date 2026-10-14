//! The request-log recorder and the superuser API over `_logs`.
//!
//! The recorder writes on a detached task, so these tests poll for an entry rather
//! than assume the insert has landed by the time the response is read.

mod support;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::{json, Value};

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

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

/// A public `posts` collection, so a record can be written without a token.
async fn seed(app: &TestApp) {
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: "posts".into(),
            collection_type: CollectionType::Base,
            fields: vec![CreateField {
                name: "title".into(),
                field_type: FieldType::Text,
                required: false,
                unique: false,
                options: json!({}),
            }],
            rules: json!({
                "listRule": "",
                "viewRule": "",
                "createRule": "",
                "updateRule": "",
                "deleteRule": "",
            }),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("create posts");
    app.reload_schema().await;
}

/// One page of logs, asserted to be a successful read.
async fn read_logs(app: &TestApp, token: &str, query: &str) -> Value {
    let reply = call_authed(
        &router(app).await,
        Method::GET,
        &format!("/api/logs?{query}"),
        None,
        token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    reply.body.expect("a list")
}

/// Poll until the log list satisfies `predicate`, or fail.
async fn wait_for_logs(
    app: &TestApp,
    token: &str,
    query: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    for _ in 0..200 {
        let body = read_logs(app, token, query).await;
        if predicate(&body) {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no log entry matched {query:?} in time");
}

#[tokio::test]
async fn every_logs_call_needs_a_superuser() {
    let Some(app) = test_app("logs_auth").await else {
        return;
    };

    for (method, uri) in [
        (Method::GET, "/api/logs"),
        (Method::GET, "/api/logs/stats"),
        (Method::GET, "/api/logs/collections"),
        (Method::GET, "/api/logs/retention"),
        (Method::PATCH, "/api/logs/retention"),
        (Method::DELETE, "/api/logs"),
    ] {
        let reply = call(&router(&app).await, method.clone(), uri, None).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {uri}");
    }

    app.cleanup().await;
}

#[tokio::test]
async fn a_request_is_recorded_with_its_status_collection_and_body() {
    let Some(app) = test_app("logs_record").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // A successful write, so the captured body can be checked against what was sent.
    let reply = support::post(
        &router(&app).await,
        "/api/collections/posts/records",
        json!({ "title": "hello" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    // A failure, so the error message is captured too.
    let reply = support::get(
        &router(&app).await,
        "/api/collections/posts/records/missing",
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    let body = wait_for_logs(&app, &token, "collection=posts&status=2xx", |body| {
        !body["items"]
            .as_array()
            .is_some_and(|items| items.is_empty())
    })
    .await;
    let entry = &body["items"][0];
    assert_eq!(entry["method"], json!("POST"));
    assert_eq!(entry["collection"], json!("posts"));
    assert_eq!(entry["status"], json!(201));
    assert!(entry["duration"].is_number(), "{entry}");
    assert!(entry["created_at"].is_string(), "{entry}");
    assert!(
        entry["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("/records")),
        "{entry}"
    );

    // The body was captured, and it is the JSON that was sent.
    let stored: Value =
        serde_json::from_str(entry["body"].as_str().expect("a body")).expect("the body is JSON");
    assert_eq!(stored["title"], json!("hello"));

    let body = wait_for_logs(&app, &token, "status=4xx", |body| {
        !body["items"]
            .as_array()
            .is_some_and(|items| items.is_empty())
    })
    .await;
    assert_eq!(body["items"][0]["status"], json!(404));
    assert!(
        body["items"][0]["error"].is_string(),
        "a failed request should record its message: {}",
        body["items"][0]
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_password_in_a_request_body_is_redacted() {
    let Some(app) = test_app("logs_redact").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // A wrong-password login: the body carries `password`, and the request is
    // recorded whatever its outcome.
    support::post(
        &router(&app).await,
        "/api/collections/_superusers/auth-with-password",
        json!({ "identity": "admin@example.com", "password": "hunter2" }),
    )
    .await;

    let body = wait_for_logs(&app, &token, "collection=_superusers", |body| {
        !body["items"]
            .as_array()
            .is_some_and(|items| items.is_empty())
    })
    .await;
    let stored = body["items"][0]["body"].as_str().expect("a body");
    assert!(stored.contains("[REDACTED]"), "{stored}");
    assert!(
        !stored.contains("hunter2"),
        "the password must never reach the log: {stored}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn retention_can_be_set_read_and_disabled() {
    let Some(app) = test_app("logs_retention").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Off by default.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/logs/retention",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.body.expect("retention")["days"], json!(null));

    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/logs/retention",
        Some(json!({ "days": 30 })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("retention")["days"], json!(30));

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/logs/retention",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.body.expect("retention")["days"], json!(30));

    // `0` disables auto-clean, which is what the Studio's "off" option sends.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/logs/retention",
        Some(json!({ "days": 0 })),
        &token,
    )
    .await;
    assert_eq!(reply.body.expect("retention")["days"], json!(null));

    app.cleanup().await;
}

#[tokio::test]
async fn logs_can_be_trimmed_by_age_and_cleared() {
    let Some(app) = test_app("logs_trim").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Insert directly so the ages are controlled exactly.
    sql(
        &app,
        "insert into _logs (method, path, status, duration_ms, created) values \
         ('GET', '/old', 200, 5, now() - interval '10 days'), \
         ('GET', '/new', 200, 5, now())",
    )
    .await;

    // `?days=5` deletes the old entry and only that one.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/logs?days=5",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a result");
    assert_eq!(body["deleted"], json!(1));
    assert_eq!(body["days"], json!(5));

    // `?days=0` deletes nothing.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/logs?days=0",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.body.expect("a result")["deleted"], json!(0));

    // `?all=true` clears. Asserted on the response: the clearing request's own log
    // entry lands asynchronously afterwards, so a row count here would race it.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/logs?all=true",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("a result")["all"], json!(true));

    app.cleanup().await;
}

#[tokio::test]
async fn stats_returns_a_zero_filled_series_for_the_window() {
    let Some(app) = test_app("logs_stats").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/logs/stats?range=24h",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("stats");
    assert_eq!(body["range"], json!("24h"));

    let series = body["series"].as_array().expect("a series");
    assert!(
        series.len() >= 24,
        "the window is zero-filled, got {} points",
        series.len()
    );
    for point in series {
        assert!(point["date"].is_string(), "{point}");
        assert!(point["total"].is_number(), "{point}");
        assert!(point["errors"].is_number(), "{point}");
        assert!(point["avg_duration"].is_number(), "{point}");
    }
    assert!(body["summary"]["requests"].is_number(), "{body}");

    // An unknown range falls back to 24h rather than reaching the SQL.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/logs/stats?range=1%20year';%20drop%20table%20_logs;%20--",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("stats")["range"], json!("24h"));

    app.cleanup().await;
}

#[tokio::test]
async fn collections_lists_the_distinct_names_it_has_seen() {
    let Some(app) = test_app("logs_collections").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    sql(
        &app,
        "insert into _logs (method, path, status, duration_ms, collection) values \
         ('GET', '/api/collections/posts/records', 200, 1, 'posts'), \
         ('GET', '/api/collections/users/records', 200, 1, 'users'), \
         ('GET', '/api/health', 200, 1, null)",
    )
    .await;

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/logs/collections",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("collections");
    let items = body["items"].as_array().expect("items");
    // `_superusers` may also appear: obtaining the token logs in over HTTP.
    assert!(items.contains(&json!("posts")), "{body}");
    assert!(items.contains(&json!("users")), "{body}");
    assert!(items.iter().all(|item| item.is_string()), "{body}");

    app.cleanup().await;
}
