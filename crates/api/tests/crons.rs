//! The cron admin API.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::{json, Value};

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// A minimal valid job.
fn job(name: &str) -> Value {
    json!({
        "name": name,
        "expression": "0 0 * * *",
        "timezone": "UTC",
        "enabled": true,
        "action": "http",
        "config": { "url": "https://example.com/hook", "method": "POST" },
    })
}

#[tokio::test]
async fn every_crons_call_needs_a_superuser() {
    let Some(app) = test_app("crons_auth").await else {
        return;
    };

    for (method, uri, body) in [
        (Method::GET, "/api/crons", None),
        (Method::POST, "/api/crons", Some(job("a"))),
        (
            Method::POST,
            "/api/crons/validate",
            Some(json!({"expression": "* * * * *"})),
        ),
        (Method::GET, "/api/crons/x", None),
        (
            Method::PATCH,
            "/api/crons/x",
            Some(json!({"enabled": false})),
        ),
        (Method::POST, "/api/crons/x", None),
        (Method::DELETE, "/api/crons/x", None),
    ] {
        let reply = call(&router(&app).await, method.clone(), uri, body).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {uri}");
    }

    app.cleanup().await;
}

#[tokio::test]
async fn a_job_can_be_created_read_updated_and_deleted() {
    let Some(app) = test_app("crons_crud").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/crons",
        Some(job("nightly")),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let created = reply.body.expect("a job");
    let id = created["id"].as_str().expect("an id").to_string();
    assert_eq!(created["name"], json!("nightly"));
    assert_eq!(created["action"], json!("http"));
    assert_eq!(created["enabled"], json!(true));
    // The schedule is computed on write, so the client can show the next run.
    assert!(created["next_run_at"].is_string(), "{created}");
    assert_eq!(created["last_status"], json!(null));

    // It is listed.
    let reply = call_authed(&router(&app).await, Method::GET, "/api/crons", None, &token).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let listed = reply.body.expect("a list");
    assert_eq!(listed["items"][0]["name"], json!("nightly"));

    // A partial update: disabling it leaves the schedule alone.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        &format!("/api/crons/{id}"),
        Some(json!({ "enabled": false })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let updated = reply.body.expect("a job");
    assert_eq!(updated["enabled"], json!(false));
    assert_eq!(updated["expression"], json!("0 0 * * *"));

    // And deleted.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        &format!("/api/crons/{id}"),
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        &format!("/api/crons/{id}"),
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    app.cleanup().await;
}

#[tokio::test]
async fn validate_previews_the_next_runs_and_refuses_a_bad_expression() {
    let Some(app) = test_app("crons_validate").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/crons/validate",
        Some(json!({ "expression": "0 0 * * *", "timezone": "UTC" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a preview");
    assert_eq!(body["valid"], json!(true));
    assert_eq!(body["nextRuns"].as_array().expect("runs").len(), 5);

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/crons/validate",
        Some(json!({ "expression": "not a cron", "timezone": "UTC" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a preview");
    assert_eq!(body["valid"], json!(false));
    assert!(body["error"].is_string(), "{body}");

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/crons/validate",
        Some(json!({ "expression": "0 0 * * *", "timezone": "Mars/Olympus" })),
        &token,
    )
    .await;
    let body = reply.body.expect("a preview");
    assert_eq!(body["valid"], json!(false));

    app.cleanup().await;
}

#[tokio::test]
async fn an_unrunnable_job_is_refused_before_it_is_stored() {
    let Some(app) = test_app("crons_refuse").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // A bad expression, an unknown action, and an http job with no url: each is a
    // 400, so a job that could never run is never persisted.
    for bad in [
        json!({ "name": "a", "expression": "nope", "action": "http", "config": {"url": "x"} }),
        json!({ "name": "b", "expression": "0 0 * * *", "action": "shell", "config": {} }),
        json!({ "name": "c", "expression": "0 0 * * *", "action": "http", "config": {} }),
    ] {
        let reply = call_authed(
            &router(&app).await,
            Method::POST,
            "/api/crons",
            Some(bad.clone()),
            &token,
        )
        .await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{bad}: {:?}",
            reply.body
        );
    }

    // A duplicate name is a conflict rather than a second job.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/crons",
        Some(job("twice")),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/crons",
        Some(job("twice")),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn a_sql_job_runs_and_records_the_outcome() {
    let Some(app) = test_app("crons_run_sql").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/crons",
        Some(json!({
            "name": "ok",
            "expression": "0 0 * * *",
            "action": "sql",
            "config": { "statement": "select 1" },
        })),
        &token,
    )
    .await;
    let id = reply.body.expect("a job")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        &format!("/api/crons/{id}"),
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let run = reply.body.expect("a job");
    assert_eq!(run["last_status"], json!("ok"));
    assert!(run["last_run_at"].is_string(), "{run}");
    assert!(run["last_duration_ms"].is_number(), "{run}");

    // A statement that fails is recorded as an error, with the message, rather than
    // reported as a successful run.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/crons",
        Some(json!({
            "name": "bad",
            "expression": "0 0 * * *",
            "action": "sql",
            "config": { "statement": "select * from definitely_not_a_table" },
        })),
        &token,
    )
    .await;
    let bad = reply.body.expect("a job")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        &format!("/api/crons/{bad}"),
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let run = reply.body.expect("a job");
    assert_eq!(run["last_status"], json!("error"));
    assert!(run["last_error"].is_string(), "{run}");

    app.cleanup().await;
}

#[tokio::test]
async fn a_hook_job_refuses_to_run_rather_than_pretending() {
    let Some(app) = test_app("crons_run_hook").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/crons",
        Some(json!({
            "name": "hooky",
            "expression": "0 0 * * *",
            "action": "hook",
            "config": { "event": "record.create" },
        })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let id = reply.body.expect("a job")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        &format!("/api/crons/{id}"),
        None,
        &token,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::NOT_IMPLEMENTED,
        "a hook cron must fail loud, not silently do nothing: {:?}",
        reply.body
    );
    let message = reply.body.expect("an error")["message"]
        .as_str()
        .expect("a message")
        .to_string();
    assert!(message.contains("Track I"), "{message}");

    app.cleanup().await;
}

/// Create a job through the API and put its schedule in the past.
///
/// The admin API always stores a *future* `next_run_at`, so this is the only way
/// to reach the state the scheduler acts on.
async fn due(app: &TestApp, token: &str, body: Value) -> String {
    let reply = call_authed(
        &router(app).await,
        Method::POST,
        "/api/crons",
        Some(body),
        token,
    )
    .await;
    let id = reply.body.expect("a job")["id"]
        .as_str()
        .expect("an id")
        .to_string();

    app.client()
        .await
        .execute(
            "update _crons set next_run_at = now() - interval '1 minute' where id = $1",
            &[&id],
        )
        .await
        .expect("the schedule moves into the past");
    id
}

/// One job's recorded outcome, read straight from the table.
async fn recorded(app: &TestApp, id: &str) -> (Option<String>, Option<String>) {
    let row = app
        .client()
        .await
        .query_one(
            "select last_status, last_error, \
                    (next_run_at > now()) as advanced from _crons where id = $1",
            &[&id],
        )
        .await
        .expect("the job is still there");
    let advanced: bool = row.get("advanced");
    assert!(advanced, "{id} did not have its schedule moved forward");
    (row.get("last_status"), row.get("last_error"))
}

#[tokio::test]
async fn the_scheduler_fires_a_due_job_and_moves_its_schedule() {
    let Some(app) = test_app("crons_sched_fires").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // A statement with an observable effect, so the test proves the action ran
    // rather than only that a row was updated.
    let id = due(
        &app,
        &token,
        json!({
            "name": "touch",
            "expression": "0 0 * * *",
            "action": "sql",
            "config": { "statement": "create table if not exists sched_probe (id int)" },
        }),
    )
    .await;

    let ran = lazypock_api::crons::run_due(&app.state, 10)
        .await
        .expect("a scheduling pass");
    assert_eq!(ran, 1, "one job was due");

    let (status, error) = recorded(&app, &id).await;
    assert_eq!(status.as_deref(), Some("ok"), "{error:?}");

    let there: bool = app
        .client()
        .await
        .query_one(
            "select to_regclass('sched_probe') is not null as there",
            &[],
        )
        .await
        .expect("a row")
        .get("there");
    assert!(there, "the job's statement did not run");

    // The schedule moved forward, so a second pass has nothing to do. Without
    // this the job would fire on every tick.
    let again = lazypock_api::crons::run_due(&app.state, 10)
        .await
        .expect("a second pass");
    assert_eq!(again, 0, "a job fired twice");

    app.cleanup().await;
}

#[tokio::test]
async fn the_scheduler_leaves_jobs_that_are_not_due_or_not_enabled() {
    let Some(app) = test_app("crons_sched_idle").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Created through the API, so `next_run_at` is in the future.
    due(
        &app,
        &token,
        json!({
            "name": "later",
            "expression": "0 0 * * *",
            "action": "sql",
            "config": { "statement": "select 1" },
        }),
    )
    .await;
    app.client()
        .await
        .execute(
            "update _crons set next_run_at = now() + interval '1 day' where name = 'later'",
            &[],
        )
        .await
        .expect("the schedule moves into the future");

    let disabled = due(
        &app,
        &token,
        json!({
            "name": "off",
            "expression": "0 0 * * *",
            "enabled": false,
            "action": "sql",
            "config": { "statement": "select 1" },
        }),
    )
    .await;

    let ran = lazypock_api::crons::run_due(&app.state, 10)
        .await
        .expect("a scheduling pass");
    assert_eq!(ran, 0);

    let (status, _) = recorded_unadvanced(&app, &disabled).await;
    assert_eq!(status, None, "a disabled job ran");

    app.cleanup().await;
}

/// Like [`recorded`], for a job the pass was expected to leave alone.
async fn recorded_unadvanced(app: &TestApp, id: &str) -> (Option<String>, Option<String>) {
    let row = app
        .client()
        .await
        .query_one(
            "select last_status, last_error from _crons where id = $1",
            &[&id],
        )
        .await
        .expect("the job is still there");
    (row.get("last_status"), row.get("last_error"))
}

#[tokio::test]
async fn a_failing_action_is_recorded_and_the_schedule_still_advances() {
    let Some(app) = test_app("crons_sched_fails").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let id = due(
        &app,
        &token,
        json!({
            "name": "bad",
            "expression": "0 0 * * *",
            "action": "sql",
            "config": { "statement": "select * from definitely_not_a_table" },
        }),
    )
    .await;

    let ran = lazypock_api::crons::run_due(&app.state, 10)
        .await
        .expect("a scheduling pass");
    assert_eq!(ran, 1);

    let (status, error) = recorded(&app, &id).await;
    assert_eq!(status.as_deref(), Some("error"));
    assert!(error.is_some(), "the failure was recorded without a reason");

    app.cleanup().await;
}

/// A job whose action cannot run must still leave a trace.
///
/// `POST /api/crons/{id}` returns the refusal to the caller, but a scheduled run
/// has no caller — so without this the schedule would advance silently and the
/// job would look like it had worked.
#[tokio::test]
async fn a_hook_job_is_recorded_as_an_error_rather_than_advancing_silently() {
    let Some(app) = test_app("crons_sched_hook").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let id = due(
        &app,
        &token,
        json!({
            "name": "hooky",
            "expression": "0 0 * * *",
            "action": "hook",
            "config": { "event": "nightly" },
        }),
    )
    .await;

    let ran = lazypock_api::crons::run_due(&app.state, 10)
        .await
        .expect("a scheduling pass");
    assert_eq!(ran, 1);

    let (status, error) = recorded(&app, &id).await;
    assert_eq!(status.as_deref(), Some("error"));
    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("hook")),
        "the refusal should say what is unsupported, got {error:?}"
    );

    app.cleanup().await;
}
