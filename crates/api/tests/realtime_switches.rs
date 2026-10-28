//! The switches that decide whether a collection publishes realtime change events.
//!
//! Both switches act on the change-event trigger itself rather than on delivery, so
//! each assertion here reads `pg_trigger` directly: if the trigger is gone, a direct
//! `INSERT` from `psql` costs nothing either — which is the point, because that
//! write never passes through the server.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::json;

use support::{call_authed, superuser_token, test_app, TestApp};

const TRIGGER: &str = "lazypock_change";

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// A minimal collection definition.
fn definition(name: &str, options: serde_json::Value) -> serde_json::Value {
    json!({
        "name": name,
        "type": "base",
        "fields": [
            { "name": "title", "type": "text", "required": false, "unique": false, "options": {} }
        ],
        "rules": { "listRule": "", "viewRule": "" },
        "options": options
    })
}

/// Whether the collection's table carries the change-event trigger.
/// Whether the collection's table carries the change-event trigger.
///
/// Scoped to this test's schema: a leftover schema from an earlier run would
/// otherwise make the count ambiguous.
async fn has_trigger(app: &TestApp, collection: &str) -> bool {
    let client = app.client().await;
    let row = client
        .query_one(
            "select count(*) from pg_trigger t \
             join pg_class c on c.oid = t.tgrelid \
             join pg_namespace n on n.oid = c.relnamespace \
             where n.nspname = current_schema() and c.relname = $1 \
             and t.tgname = $2 and not t.tgisinternal",
            &[&collection, &TRIGGER],
        )
        .await
        .expect("read pg_trigger");
    row.get::<_, i64>(0) == 1
}

/// Create a collection as a superuser, asserting the call succeeded.
async fn create(app: &TestApp, name: &str, options: serde_json::Value, token: &str) {
    let reply = call_authed(
        &router(app).await,
        Method::POST,
        "/api/collections",
        Some(definition(name, options)),
        token,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::CREATED,
        "creating {name} failed: {:?}",
        reply.body
    );
    // The running node refreshes its cache from `lazypock_schema_changed`; a test has
    // no listener running, so it asks.
    app.reload_schema().await;
}

async fn patch_collection(app: &TestApp, name: &str, body: serde_json::Value, token: &str) {
    let reply = call_authed(
        &router(app).await,
        Method::PATCH,
        &format!("/api/collections/{name}"),
        Some(body),
        token,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "patching {name} failed: {:?}",
        reply.body
    );
}

async fn set_instance_switch(app: &TestApp, enabled: bool, token: &str) {
    let reply = call_authed(
        &router(app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "realtime": enabled })),
        token,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "setting the instance switch failed: {:?}",
        reply.body
    );
}

/// A collection publishes by default — the behaviour before the option existed.
#[tokio::test]
async fn a_collection_publishes_unless_it_opts_out() {
    let Some(app) = test_app("realtime_switch_default").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    create(&app, "published", json!({}), &token).await;
    create(&app, "opted_out", json!({ "realtime": false }), &token).await;

    assert!(
        has_trigger(&app, "published").await,
        "a collection that says nothing about realtime must publish"
    );
    assert!(
        !has_trigger(&app, "opted_out").await,
        "realtime: false must leave no trigger at all, not a trigger that filters"
    );

    app.cleanup().await;
}

/// The switch is part of the definition, so it takes effect on the next save rather
/// than on the next collection.
#[tokio::test]
async fn turning_realtime_off_removes_the_trigger_and_on_restores_it() {
    let Some(app) = test_app("realtime_switch_update").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    create(&app, "posts", json!({}), &token).await;
    assert!(has_trigger(&app, "posts").await);

    patch_collection(
        &app,
        "posts",
        json!({ "options": { "realtime": false } }),
        &token,
    )
    .await;
    assert!(
        !has_trigger(&app, "posts").await,
        "turning the switch off must drop the trigger"
    );

    patch_collection(
        &app,
        "posts",
        json!({ "options": { "realtime": true } }),
        &token,
    )
    .await;
    assert!(
        has_trigger(&app, "posts").await,
        "turning it back on must restore publishing"
    );

    app.cleanup().await;
}

/// `options` is free-form, so a malformed switch is refused instead of being read as
/// "on" — a setting that silently does nothing is worse than a rejected request.
#[tokio::test]
async fn a_realtime_option_that_is_not_a_boolean_is_refused() {
    let Some(app) = test_app("realtime_switch_invalid").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections",
        Some(definition("posts", json!({ "realtime": "off" }))),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    app.cleanup().await;
}

/// The instance-wide switch has to reach collections that already exist, or it would
/// only affect the ones created after it — which is not what an operator turning
/// realtime off is asking for.
#[tokio::test]
async fn the_instance_switch_reaches_existing_and_future_collections() {
    let Some(app) = test_app("realtime_switch_instance").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    create(&app, "posts", json!({}), &token).await;
    create(&app, "comments", json!({}), &token).await;
    assert!(has_trigger(&app, "posts").await);
    assert!(has_trigger(&app, "comments").await);

    set_instance_switch(&app, false, &token).await;
    assert!(
        !has_trigger(&app, "posts").await,
        "an existing collection must stop publishing"
    );
    assert!(!has_trigger(&app, "comments").await);

    // A collection created while the switch is off does not get a trigger it would
    // then have to be told to remove.
    create(&app, "later", json!({}), &token).await;
    assert!(
        !has_trigger(&app, "later").await,
        "a collection created while realtime is off must not publish"
    );

    set_instance_switch(&app, true, &token).await;
    assert!(has_trigger(&app, "posts").await);
    assert!(has_trigger(&app, "comments").await);
    assert!(
        has_trigger(&app, "later").await,
        "turning realtime back on must restore every collection"
    );

    app.cleanup().await;
}

/// A collection that opted out stays out when the instance switch goes back on.
#[tokio::test]
async fn the_two_switches_compose() {
    let Some(app) = test_app("realtime_switch_compose").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    create(&app, "opted_out", json!({ "realtime": false }), &token).await;
    create(&app, "published", json!({}), &token).await;

    set_instance_switch(&app, false, &token).await;
    set_instance_switch(&app, true, &token).await;

    assert!(
        !has_trigger(&app, "opted_out").await,
        "the instance switch coming back on must not override a collection that opted out"
    );
    assert!(has_trigger(&app, "published").await);

    app.cleanup().await;
}

/// `_rt_log` is what a resuming client is replayed from, so the roles that can run
/// SQL as a request must not be able to write it. The trigger writes it now, as its
/// owner, which is why the grant could go.
#[tokio::test]
async fn the_request_roles_cannot_write_the_change_log() {
    let Some(app) = test_app("realtime_log_privileges").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    create(&app, "posts", json!({}), &token).await;

    for role in ["lazypock_anon", "lazypock_authenticated"] {
        let mut client = app.client().await;
        let transaction = client.transaction().await.expect("transaction");
        transaction
            .batch_execute(&format!("set local role {role}"))
            .await
            .expect("assume role");

        let forged = transaction
            .execute(
                "insert into _rt_log (collection, op, record_id, origin) \
                 values ('posts', 'create', 'forged', null)",
                &[],
            )
            .await;
        assert!(
            forged.is_err(),
            "{role} must not be able to append to the change log"
        );
    }

    // A real write still lands in the log: the trigger does it as its owner.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections/posts/records",
        Some(json!({ "title": "hello" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    let client = app.client().await;
    let row = client
        .query_one(
            "select count(*) from _rt_log where collection = 'posts' and op = 'create'",
            &[],
        )
        .await
        .expect("count log rows");
    assert!(
        row.get::<_, i64>(0) >= 1,
        "the trigger must still record a change now that the roles cannot"
    );

    app.cleanup().await;
}

/// The log grows with every write, including writes that never went through the
/// server, so retention is a day count and older rows go.
#[tokio::test]
async fn retention_drops_old_change_events_and_keeps_recent_ones() {
    let Some(app) = test_app("realtime_log_retention").await else {
        return;
    };

    let client = app.client().await;
    client
        .batch_execute(
            "insert into _rt_log (collection, op, record_id, origin, created) values \
             ('posts', 'create', 'stale', null, now() - interval '30 days'), \
             ('posts', 'create', 'fresh', null, now())",
        )
        .await
        .expect("seed the log");

    let deleted = lazypock_db::realtime::prune_log(&client, 7)
        .await
        .expect("prune");
    assert_eq!(deleted, 1, "only the row outside the window should go");

    let remaining: Vec<String> = client
        .query("select record_id from _rt_log", &[])
        .await
        .expect("read the log")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(remaining, vec!["fresh".to_string()]);

    // `0` keeps everything, which is the behaviour before retention existed.
    assert_eq!(
        lazypock_db::realtime::prune_log(&client, 0)
            .await
            .expect("prune with retention off"),
        0
    );

    app.cleanup().await;
}
