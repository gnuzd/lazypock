//! HTTP-level tests for `POST /api/batch`.

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{get, post, test_app, TestApp};

fn text_field(name: &str, required: bool, unique: bool) -> CreateField {
    CreateField {
        name: name.into(),
        field_type: FieldType::Text,
        required,
        unique,
        options: json!({}),
    }
}

async fn seed(app: &TestApp) {
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: "posts".into(),
            collection_type: CollectionType::Base,
            fields: vec![
                text_field("title", true, false),
                text_field("slug", false, true),
            ],
            rules: json!({}),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("posts");
    app.reload_schema().await;
}

async fn batch(app: &TestApp, requests: Value) -> support::Reply {
    let router = lazypock_api::router::router(app.state.clone());
    post(&router, "/api/batch", json!({ "requests": requests })).await
}

async fn record_count(app: &TestApp) -> i64 {
    let router = lazypock_api::router::router(app.state.clone());
    let reply = get(&router, "/api/collections/posts/records?perPage=1").await;
    reply.body.expect("body")["totalItems"]
        .as_i64()
        .expect("count")
}

#[tokio::test]
async fn a_batch_applies_every_request_in_order() {
    let Some(app) = test_app("batch_success").await else {
        return;
    };
    seed(&app).await;

    let reply = batch(
        &app,
        json!([
            {"method": "POST", "url": "/api/collections/posts/records", "body": {"title": "One"}},
            {"method": "POST", "url": "/api/collections/posts/records", "body": {"title": "Two"}},
        ]),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let results = reply.body.expect("body");
    let results = results.as_array().expect("array of results");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["status"], json!(201));
    assert_eq!(results[0]["body"]["title"], json!("One"));
    assert_eq!(results[1]["status"], json!(201));
    assert_eq!(results[1]["body"]["title"], json!("Two"));

    assert_eq!(record_count(&app).await, 2);

    app.cleanup().await;
}

#[tokio::test]
async fn a_batch_combines_create_update_and_delete() {
    let Some(app) = test_app("batch_crud").await else {
        return;
    };
    let router = lazypock_api::router::router(app.state.clone());
    seed(&app).await;

    let created = post(
        &router,
        "/api/collections/posts/records",
        json!({"title": "Original", "slug": "s1"}),
    )
    .await
    .body
    .expect("body");
    let id = created["id"].as_str().expect("id").to_string();

    let reply = batch(
        &app,
        json!([
            {"method": "PATCH", "url": format!("/api/collections/posts/records/{id}"), "body": {"title": "Updated"}},
            {"method": "GET", "url": format!("/api/collections/posts/records/{id}")},
            {"method": "POST", "url": "/api/collections/posts/records", "body": {"title": "Another", "slug": "s2"}},
            {"method": "DELETE", "url": format!("/api/collections/posts/records/{id}")},
        ]),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let results = reply.body.expect("body");
    let results = results.as_array().expect("results");

    assert_eq!(results[0]["status"], json!(200));
    assert_eq!(results[0]["body"]["title"], json!("Updated"));
    assert_eq!(results[1]["status"], json!(200));
    assert_eq!(
        results[1]["body"]["title"],
        json!("Updated"),
        "the patch is visible inside the batch"
    );
    assert_eq!(results[2]["status"], json!(201));
    assert_eq!(results[3]["status"], json!(204));
    assert_eq!(results[3]["body"], json!(null));

    // Only the second create survives.
    assert_eq!(record_count(&app).await, 1);

    app.cleanup().await;
}

#[tokio::test]
async fn a_failing_request_rolls_the_whole_batch_back() {
    let Some(app) = test_app("batch_rollback_validation").await else {
        return;
    };
    seed(&app).await;

    let reply = batch(
        &app,
        json!([
            {"method": "POST", "url": "/api/collections/posts/records", "body": {"title": "Kept?"}},
            {"method": "POST", "url": "/api/collections/posts/records", "body": {"slug": "no-title"}},
        ]),
    )
    .await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let body = reply.body.expect("body");
    assert!(
        body["message"]
            .as_str()
            .expect("message")
            .contains("batch request 1"),
        "the failing index should be named: {body}"
    );
    // The per-field detail survives the annotation.
    assert_eq!(
        body["data"]["title"]["code"],
        json!("validation_required"),
        "{body}"
    );

    // Nothing was written: the first create was rolled back with the batch.
    assert_eq!(record_count(&app).await, 0);

    app.cleanup().await;
}

#[tokio::test]
async fn a_database_error_also_rolls_the_batch_back() {
    let Some(app) = test_app("batch_rollback_unique").await else {
        return;
    };
    seed(&app).await;

    let reply = batch(
        &app,
        json!([
            {"method": "POST", "url": "/api/collections/posts/records", "body": {"title": "First", "slug": "same"}},
            {"method": "POST", "url": "/api/collections/posts/records", "body": {"title": "Second", "slug": "same"}},
        ]),
    )
    .await;

    assert_eq!(reply.status, StatusCode::CONFLICT, "{:?}", reply.body);
    assert_eq!(
        record_count(&app).await,
        0,
        "the first insert must be rolled back"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn batches_are_bounded() {
    let Some(app) = test_app("batch_bounds").await else {
        return;
    };
    seed(&app).await;

    let empty = batch(&app, json!([])).await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);
    assert!(empty.body.expect("body")["message"]
        .as_str()
        .unwrap()
        .contains("at least one"));

    let many: Vec<Value> = (0..lazypock_api::batch::MAX_BATCH_REQUESTS + 1)
        .map(|index| {
            json!({
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": {"title": format!("Row {index}")},
            })
        })
        .collect();
    let reply = batch(&app, Value::Array(many)).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert!(
        reply.body.expect("body")["message"]
            .as_str()
            .unwrap()
            .contains("at most"),
        "the limit should be named"
    );
    assert_eq!(
        record_count(&app).await,
        0,
        "an oversized batch writes nothing"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn unsupported_requests_are_refused_before_anything_is_written() {
    let Some(app) = test_app("batch_unsupported").await else {
        return;
    };
    seed(&app).await;

    let cases: &[(Value, StatusCode, &str)] = &[
        (
            json!([{"method": "OPTIONS", "url": "/api/collections/posts/records"}]),
            StatusCode::NOT_IMPLEMENTED,
            "not supported in a batch",
        ),
        (
            json!([{"method": "POST", "url": "/api/collections"}]),
            StatusCode::BAD_REQUEST,
            "is not a record URL",
        ),
        (
            json!([{"method": "POST", "url": "/api/collections/posts/fields"}]),
            StatusCode::BAD_REQUEST,
            "is not a record URL",
        ),
        (
            json!([{"method": "POST", "url": "/api/collections/posts/records?fields=id"}]),
            StatusCode::NOT_IMPLEMENTED,
            "query parameters",
        ),
        (
            json!([{"method": "POST", "url": "/api/collections/nope/records", "body": {"title": "x"}}]),
            StatusCode::NOT_FOUND,
            "no collection",
        ),
        (
            json!([{"method": "PATCH", "url": "/api/collections/posts/records", "body": {"title": "x"}}]),
            StatusCode::BAD_REQUEST,
            "needs a record id",
        ),
        (
            json!([{"method": "GET", "url": "/api/collections/posts/records"}]),
            StatusCode::BAD_REQUEST,
            "needs a record id",
        ),
        (
            json!([{"method": "POST", "url": "/api/collections/posts/records/abcDEF123456789", "body": {"title": "x"}}]),
            StatusCode::BAD_REQUEST,
            "without a record id",
        ),
        (
            json!([{"method": "POST", "url": "/api/collections/posts/records"}]),
            StatusCode::BAD_REQUEST,
            "needs a request body",
        ),
        (
            json!([{"method": "POST", "url": "/api/collections/posts/records", "body": "nope"}]),
            StatusCode::BAD_REQUEST,
            "must be a JSON object",
        ),
    ];

    for (requests, expected, needle) in cases {
        let reply = batch(&app, requests.clone()).await;
        assert_eq!(reply.status, *expected, "for {requests}");
        let body = reply.body.expect("body");
        assert!(
            body["message"].as_str().unwrap().contains(needle),
            "for {requests}: {body}"
        );
    }

    assert_eq!(record_count(&app).await, 0);
    app.cleanup().await;
}

#[tokio::test]
async fn a_missing_record_rolls_back_and_names_the_index() {
    let Some(app) = test_app("batch_missing_record").await else {
        return;
    };
    seed(&app).await;

    let reply = batch(
        &app,
        json!([
            {"method": "POST", "url": "/api/collections/posts/records", "body": {"title": "Kept?"}},
            {"method": "DELETE", "url": "/api/collections/posts/records/zzzzzzzzzzzzzzz"},
        ]),
    )
    .await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{:?}", reply.body);
    let body = reply.body.expect("body");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("batch request 1"),
        "{body}"
    );
    assert_eq!(record_count(&app).await, 0);

    app.cleanup().await;
}
