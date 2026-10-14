//! The large-import confirmation gate.
//!
//! **Its own test binary on purpose.** The gate is configured by
//! `LAZYPOCK_IMPORT_CONFIRM_RECORDS`, and `set_var` changes the whole process — so in
//! a file shared with other tests, the ones that do not set it would read the value
//! this one left behind and fail intermittently. Cargo runs each integration test
//! file as a separate process, which is the isolation this needs.

mod support;

use axum::body::Body;
use axum::http::{header, Method, Request};
use serde_json::json;
use tower::ServiceExt;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{superuser_token, test_app, TestApp};

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
            rules: json!({}),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("create posts");
    app.reload_schema().await;

    let client = app.client().await;
    client
        .batch_execute("insert into posts (id, title) values ('p1', 'first'), ('p2', 'second')")
        .await
        .expect("seed");
}

/// Fetch an archive, then upload it back with no confirmation.
async fn export_then_import(app: &TestApp, token: &str) -> support::Reply {
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/export/archive")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let response = app.state.clone();
    let archive = axum::body::to_bytes(
        lazypock_api::router::router(response)
            .oneshot(request)
            .await
            .expect("response")
            .into_body(),
        16 * 1024 * 1024,
    )
    .await
    .expect("body")
    .to_vec();

    let boundary = "----lazypockGateBoundary";
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"backup.zip\"\r\nContent-Type: application/zip\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&archive);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/import")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .expect("request");
    let response = lazypock_api::router::router(app.state.clone())
        .oneshot(request)
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("body");
    support::Reply {
        status,
        headers: Default::default(),
        body: if bytes.is_empty() {
            None
        } else {
            serde_json::from_slice(&bytes).ok()
        },
    }
}

#[tokio::test]
async fn a_large_import_is_gated_until_confirmed() {
    let Some(app) = test_app("backup_gate_isolated").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Any import needs confirming, so the gate is reachable without a huge fixture.
    std::env::set_var("LAZYPOCK_IMPORT_CONFIRM_RECORDS", "0");

    let reply = export_then_import(&app, &token).await;
    assert_eq!(
        reply.status,
        axum::http::StatusCode::CONFLICT,
        "the gate is a conflict, not a failure: {:?}",
        reply.body
    );

    let body = reply.body.expect("a gate");
    assert_eq!(body["data"]["requires_confirmation"], json!(true));
    assert_eq!(body["data"]["preflight"]["total_records"], json!(2));
    assert!(
        body["data"]["preflight"]["collections"].is_array(),
        "the preflight names what would be written: {body}"
    );

    // Nothing was written before the confirmation.
    let client = app.client().await;
    let count: i64 = client
        .query_one("select count(*) from posts", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, 2, "the gate must not have applied anything");

    app.cleanup().await;
}
