//! End-to-end tests: `$app` inside a hook, under the caller's rules.
//!
//! Every case here is about the *context*, not about the JavaScript surface: a hook
//! fired by an anonymous request must reach the database as anonymous, a write to the
//! collection whose hook is running must be refused, and mail must be queued rather
//! than sent inside the dispatch.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::{json, Value};

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// Create one base collection with a `note` field and the given rules.
async fn seed_collection(app: &TestApp, name: &str, rules: Value) {
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: name.into(),
            collection_type: CollectionType::Base,
            fields: vec![CreateField {
                name: "note".into(),
                field_type: FieldType::Text,
                required: false,
                unique: false,
                options: json!({}),
            }],
            rules,
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .unwrap_or_else(|err| panic!("create {name}: {err}"));
}

/// A collection nobody anonymous may read or write.
async fn seed_locked(app: &TestApp, name: &str) {
    seed_collection(
        app,
        name,
        json!({
            "listRule": json!(null), "viewRule": json!(null),
            "createRule": "@request.auth.id != \"\"",
            "updateRule": json!(null), "deleteRule": json!(null),
        }),
    )
    .await;
}

/// A collection anonymous writes are allowed to reach.
async fn seed_open(app: &TestApp, name: &str) {
    seed_collection(
        app,
        name,
        json!({
            "listRule": "", "viewRule": "", "createRule": "",
            "updateRule": "", "deleteRule": "",
        }),
    )
    .await;
}

/// The three collections every test here needs.
async fn seed(app: &TestApp) {
    seed_open(app, "orders").await;
    seed_open(app, "audit_open").await;
    seed_locked(app, "secrets").await;
    seed_locked(app, "audit_locked").await;
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

/// Create one record as the superuser, who bypasses every rule.
async fn seed_record(app: &Router, token: &str, collection: &str, note: &str) -> String {
    let reply = call_authed(
        app,
        Method::POST,
        &format!("/api/collections/{collection}/records"),
        Some(json!({ "note": note })),
        token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body.expect("a record")["id"]
        .as_str()
        .expect("an id")
        .to_string()
}

/// The `message` of an error reply.
fn message(reply: &support::Reply) -> String {
    reply
        .body
        .as_ref()
        .and_then(|body| body.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn a_hooks_read_is_matched_against_the_callers_rules() {
    let Some(app) = test_app("hooks_app_read").await else {
        return;
    };
    seed(&app).await;
    let router = router(&app).await;
    let token = superuser_token(&app, "app-read@example.com", "app-read-password").await;

    // The row exists, but an anonymous caller's rules do not reveal it.
    seed_record(&router, &token, "secrets", "top-secret").await;

    install(
        &app,
        r#"
        onRecordBeforeCreate("orders", (e: any) => {
            const secret = $app.findRecordById("secrets", e.record.secretId || "");
            e.record.note = secret ? "saw:" + secret.note : "blind";
        });
        "#,
    )
    .await;

    // Find the id the superuser just created, so the hook asks for a real row.
    let listed = call_authed(
        &router,
        Method::GET,
        "/api/collections/secrets/records",
        None,
        &token,
    )
    .await;
    let secret_id = listed.body.expect("a list")["items"][0]["id"]
        .as_str()
        .expect("an id")
        .to_string();
    install(
        &app,
        &format!(
            r#"
            onRecordBeforeCreate("orders", (e: any) => {{
                const secret = $app.findRecordById("secrets", "{secret_id}");
                e.record.note = secret ? "saw:" + secret.note : "blind";
            }});
            "#
        ),
    )
    .await;

    // Anonymous: the row is there, and the hook still sees nothing.
    let reply = call(
        &router,
        Method::POST,
        "/api/collections/orders/records",
        Some(json!({})),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    assert_eq!(reply.body.expect("a record")["note"], json!("blind"));
}

#[tokio::test]
async fn a_hooks_write_is_allowed_by_the_callers_rules_and_refused_without_them() {
    let Some(app) = test_app("hooks_app_write").await else {
        return;
    };
    seed(&app).await;
    let router = router(&app).await;
    let token = superuser_token(&app, "app-write@example.com", "app-write-password").await;

    // Open target: an anonymous hook write lands.
    install(
        &app,
        r#"
        onRecordAfterCreate("orders", (e: any) => {
            $app.save("audit_open", { note: "order " + e.record.note });
        });
        "#,
    )
    .await;
    let reply = call(
        &router,
        Method::POST,
        "/api/collections/orders/records",
        Some(json!({ "note": "opened" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    let audited = call_authed(
        &router,
        Method::GET,
        "/api/collections/audit_open/records",
        None,
        &token,
    )
    .await;
    assert_eq!(
        audited.body.expect("a list")["items"][0]["note"],
        json!("order opened"),
        "the hook's write went through"
    );

    // Locked target: the same hook, the same anonymous caller, refused — and the
    // order it was reacting to is not written either, because the before-hook runs
    // inside the write's transaction.
    install(
        &app,
        r#"
        onRecordBeforeCreate("orders", (e: any) => {
            $app.save("audit_locked", { note: "order " + e.record.note });
        });
        "#,
    )
    .await;
    let refused = call(
        &router,
        Method::POST,
        "/api/collections/orders/records",
        Some(json!({ "note": "blocked" })),
    )
    .await;
    assert!(
        !refused.status.is_success(),
        "a denied hook write must fail the request: {:?}",
        refused.body
    );

    let orders = call_authed(
        &router,
        Method::GET,
        "/api/collections/orders/records",
        None,
        &token,
    )
    .await;
    assert_eq!(
        orders.body.expect("a list")["totalItems"],
        json!(1),
        "only the order the hook allowed exists"
    );
}

#[tokio::test]
async fn a_hook_cannot_write_the_collection_whose_hook_is_running() {
    let Some(app) = test_app("hooks_app_reentrancy").await else {
        return;
    };
    seed(&app).await;
    let router = router(&app).await;

    install(
        &app,
        r#"
        onRecordBeforeCreate("orders", (e: any) => {
            $app.save("orders", { note: "again" });
        });
        "#,
    )
    .await;

    let reply = call(
        &router,
        Method::POST,
        "/api/collections/orders/records",
        Some(json!({ "note": "first" })),
    )
    .await;
    assert!(
        !reply.status.is_success(),
        "a same-collection save must be refused: {:?}",
        reply.body
    );
    assert!(
        message(&reply).contains("its own hook"),
        "the refusal says why: {}",
        message(&reply)
    );
}

#[tokio::test]
async fn a_hooks_mail_is_queued_not_sent_inside_the_dispatch() {
    let Some(app) = test_app("hooks_app_mail").await else {
        return;
    };
    seed(&app).await;
    let router = router(&app).await;

    // No SMTP configured: the same refusal the endpoint gives, which proves the hook
    // goes through the endpoint's own path rather than straight at a transporter.
    install(
        &app,
        r#"
        onRecordAfterCreate("orders", (e: any) => {
            $app.mailer.send({ to: "someone@example.com", template: "verification", assigns: { token: "t" } });
        });
        "#,
    )
    .await;

    let reply = call(
        &router,
        Method::POST,
        "/api/collections/orders/records",
        Some(json!({ "note": "mail" })),
    )
    .await;
    assert!(
        !reply.status.is_success(),
        "a send with no SMTP must fail loudly: {:?}",
        reply.body
    );
    assert!(
        message(&reply).contains("mail is not configured"),
        "the message names the fix: {}",
        message(&reply)
    );
}

#[tokio::test]
async fn the_rules_a_hook_runs_under_are_the_triggering_requests() {
    let Some(app) = test_app("hooks_app_superuser").await else {
        return;
    };
    seed(&app).await;
    let router = router(&app).await;
    let token = superuser_token(&app, "app-super@example.com", "app-super-password").await;
    seed_record(&router, &token, "secrets", "top-secret").await;

    // The carve-out, stated as a test: a superuser's own request already bypasses the
    // rules, so a hook it triggers does too — the same access, not more.
    install(
        &app,
        r#"
        onRecordBeforeCreate("orders", (e: any) => {
            const secret = $app.findRecordById("secrets", e.record.secretId || "");
            e.record.note = secret ? "saw:" + secret.note : "blind";
        });
        "#,
    )
    .await;
    let listed = call_authed(
        &router,
        Method::GET,
        "/api/collections/secrets/records",
        None,
        &token,
    )
    .await;
    let secret_id = listed.body.expect("a list")["items"][0]["id"]
        .as_str()
        .expect("an id")
        .to_string();
    install(
        &app,
        &format!(
            r#"
            onRecordBeforeCreate("orders", (e: any) => {{
                const secret = $app.findRecordById("secrets", "{secret_id}");
                e.record.note = secret ? "saw:" + secret.note : "blind";
            }});
            "#
        ),
    )
    .await;

    let reply = call_authed(
        &router,
        Method::POST,
        "/api/collections/orders/records",
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    assert_eq!(
        reply.body.expect("a record")["note"],
        json!("saw:top-secret"),
        "a superuser-triggered hook runs with the superuser's access"
    );
}
