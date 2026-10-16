//! The Studio's SQL console: `POST /api/sql/query`.

mod support;

use axum::http::{Method, StatusCode};

use serde_json::json;

use axum::Router;
use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// Run a statement through the console.
async fn run(app: &TestApp, token: &str, sql: &str) -> support::Reply {
    call_authed(
        &router(app).await,
        Method::POST,
        "/api/sql/query",
        Some(json!({ "sql": sql })),
        token,
    )
    .await
}

#[tokio::test]
async fn the_console_returns_columns_and_rows_in_order() {
    let Some(app) = test_app("sql_select").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = run(&app, &token, "select 1 as the_answer, 'two' as the_text").await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let body = reply.body.expect("a result");
    assert_eq!(body["columns"], json!(["the_answer", "the_text"]));
    assert_eq!(body["rows"], json!([[1, "two"]]));

    app.cleanup().await;
}

/// A query that matches nothing still has to describe itself, or the console shows
/// an empty grid with no headers — which is the state a person is most often in
/// when they are writing a query.
#[tokio::test]
async fn a_query_with_no_rows_still_reports_its_columns() {
    let Some(app) = test_app("sql_no_rows").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = run(&app, &token, "select 1 as only_column where false").await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let body = reply.body.expect("a result");
    assert_eq!(body["columns"], json!(["only_column"]));
    assert_eq!(body["rows"], json!([]));

    app.cleanup().await;
}

#[tokio::test]
async fn a_statement_that_is_not_read_only_is_refused_by_its_keyword() {
    let Some(app) = test_app("sql_write").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = run(&app, &token, "delete from _superusers").await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let message = reply.body.expect("an error")["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        message.contains("read-only"),
        "the refusal should say why, got {message:?}"
    );

    // A trailing semicolon is what people type, and it must not turn a valid
    // statement into a rejection.
    let reply = run(&app, &token, "select 1 as ok;\n").await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    app.cleanup().await;
}

/// The keyword check is the visible half. This is the half Postgres enforces: a
/// `WITH` reads as read-only and is not, and only the read-only transaction stops
/// it.
#[tokio::test]
async fn a_write_hidden_inside_a_with_is_refused() {
    let Some(app) = test_app("sql_cte_write").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    app.client()
        .await
        .batch_execute("create table if not exists sql_probe (id int)")
        .await
        .expect("a table to write to");

    // The keyword is `with`, so the visible check lets it through.
    let reply = run(
        &app,
        &token,
        "with x as (insert into sql_probe (id) values (1) returning id) select * from x",
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    // Whatever refused it, nothing was written.
    assert_eq!(written(&app).await, 0, "the refused write left rows behind");

    app.cleanup().await;
}

/// The case only the read-only transaction can stop.
///
/// A plain `select` that calls a function which writes passes the keyword check
/// and wraps into a subquery perfectly well, so nothing in this server is what
/// refuses it — Postgres is.
#[tokio::test]
async fn a_select_that_writes_from_a_function_is_refused_by_read_only() {
    let Some(app) = test_app("sql_fn_write").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    app.client()
        .await
        .batch_execute(
            "create table if not exists sql_probe (id int); \
             create or replace function sql_write_probe() returns int language sql as \
             $$ insert into sql_probe (id) values (99) returning id $$",
        )
        .await
        .expect("a table and a function that writes to it");

    let reply = run(&app, &token, "select sql_write_probe() as wrote").await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let message = reply.body.expect("an error")["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        message.contains("read-only"),
        "Postgres should have refused the write, got {message:?}"
    );
    assert_eq!(written(&app).await, 0, "the refused write left rows behind");

    app.cleanup().await;
}

/// How many rows the probe table holds.
async fn written(app: &TestApp) -> i64 {
    app.client()
        .await
        .query_one("select count(*) as n from sql_probe", &[])
        .await
        .expect("a row")
        .get("n")
}

#[tokio::test]
async fn explain_is_allowed_and_describes_itself() {
    let Some(app) = test_app("sql_explain").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = run(&app, &token, "explain select 1").await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let body = reply.body.expect("a result");
    assert_eq!(body["columns"], json!(["QUERY PLAN"]));
    assert_eq!(
        body["rows"].as_array().map(Vec::len),
        Some(1),
        "explain returns one row for a single statement"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_query_that_does_not_parse_reports_the_database_message() {
    let Some(app) = test_app("sql_bad_syntax").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = run(&app, &token, "select * from definitely_not_a_table").await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let message = reply.body.expect("an error")["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        message.contains("definitely_not_a_table"),
        "the console should say what is wrong, got {message:?}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn the_console_needs_a_superuser() {
    let Some(app) = test_app("sql_auth").await else {
        return;
    };

    let reply = call(
        &router(&app).await,
        Method::POST,
        "/api/sql/query",
        Some(json!({ "sql": "select 1" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);

    app.cleanup().await;
}
