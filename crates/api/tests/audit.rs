//! The admin action audit log: `_audit` and `GET /api/audit`.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;

use serde_json::{json, Value};

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// `GET /api/audit`.
async fn read_log(app: &TestApp, token: &str) -> Value {
    let reply = call_authed(&router(app).await, Method::GET, "/api/audit", None, token).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    reply.body.expect("a list")
}

/// Whether any entry has this action.
fn has_action(log: &Value, action: &str) -> bool {
    log["items"]
        .as_array()
        .expect("items")
        .iter()
        .any(|item| item["action"] == json!(action))
}

#[tokio::test]
async fn an_admin_action_is_recorded_with_who_did_it() {
    let Some(app) = test_app("audit_records").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Nothing should have been recorded before the action.
    let log = read_log(&app, &token).await;
    assert_eq!(log["totalItems"], json!(0), "{log}");

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(json!({
            "name": "audited",
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

    let log = read_log(&app, &token).await;
    assert_eq!(log["totalItems"], json!(1), "{log}");

    let entry = &log["items"][0];
    assert_eq!(entry["action"], json!("collection.create"));
    assert_eq!(entry["target"], json!("audited"));
    // The actor is the superuser's record id, which is what makes the entry worth
    // keeping: it says who as well as what.
    assert_eq!(
        entry["actor"].as_str().map(str::len),
        Some(15),
        "the actor should be the superuser's id, got {}",
        entry["actor"]
    );
    assert!(entry["created"].is_string(), "{entry}");

    app.cleanup().await;
}

/// A settings write carries credentials. The log records which keys changed and
/// never their values, or the audit trail becomes one more place they leak from.
#[tokio::test]
async fn settings_values_are_not_copied_into_the_audit_log() {
    let Some(app) = test_app("audit_no_values").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let secret = "hunter2-not-in-the-log";
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "mail": { "host": "smtp.example", "password": secret } })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let log = read_log(&app, &token).await;
    assert!(has_action(&log, "settings.update"), "{log}");

    let text = log.to_string();
    assert!(
        !text.contains(secret),
        "the audit log copied a settings value: {text}"
    );
    assert!(
        text.contains("mail"),
        "the changed key should be recorded: {text}"
    );

    app.cleanup().await;
}

/// The request log is superuser-only, and this is the same kind of record: it names
/// who did what and when.
#[tokio::test]
async fn the_audit_log_needs_a_superuser() {
    let Some(app) = test_app("audit_auth").await else {
        return;
    };

    let reply = call(&router(&app).await, Method::GET, "/api/audit", None).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);

    app.cleanup().await;
}

/// A refusal must not be recorded as a change: a collection that was never created
/// has no `collection.create` entry.
#[tokio::test]
async fn a_refused_action_leaves_no_entry() {
    let Some(app) = test_app("audit_refusal").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(json!({
            "name": "not creatable",
            "type": "base",
            "fields": [],
            "rules": {},
            "options": {}
        })),
        &token,
    )
    .await;
    assert_ne!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    let log = read_log(&app, &token).await;
    assert_eq!(log["totalItems"], json!(0), "{log}");

    app.cleanup().await;
}
