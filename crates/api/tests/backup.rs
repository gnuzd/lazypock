//! Backup and restore over HTTP.

mod support;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

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

/// A `posts` collection with a couple of rows.
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
    sql(
        app,
        "insert into posts (id, title) values ('p1', 'first'), ('p2', 'second')",
    )
    .await;
}

/// Download an archive.
async fn export(app: &TestApp, token: &str) -> Vec<u8> {
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/export/archive")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let response = router(app).await.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .expect("body")
        .to_vec()
}

/// Upload an archive to the import endpoint.
async fn import(
    app: &TestApp,
    token: &str,
    archive: &[u8],
    extras: &[(&str, &str)],
) -> support::Reply {
    let boundary = "----lazypockBackupBoundary";
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"backup.zip\"\r\nContent-Type: application/zip\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(archive);
    body.extend_from_slice(b"\r\n");
    for (name, value) in extras {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

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
    let response = router(app).await.oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
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
async fn backup_and_restore_need_a_superuser() {
    let Some(app) = test_app("backup_auth").await else {
        return;
    };

    for (method, uri) in [
        (Method::GET, "/api/export/archive"),
        (Method::GET, "/api/import/preflight"),
        (Method::POST, "/api/import"),
    ] {
        let reply = call(&router(&app).await, method.clone(), uri, None).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {uri}");
    }

    app.cleanup().await;
}

#[tokio::test]
async fn a_database_can_be_exported_and_imported_back_over_http() {
    let Some(app) = test_app("backup_http_round_trip").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let archive = export(&app, &token).await;
    assert_eq!(&archive[0..2], b"PK", "the download is a zip");

    // Wipe, then restore through the endpoint.
    sql(&app, "delete from posts").await;

    let reply = import(&app, &token, &archive, &[("atomic", "batch")]).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let report = reply.body.expect("a report");
    assert_eq!(report["imported"][0]["name"], json!("posts"));
    assert_eq!(report["imported"][0]["records_imported"], json!(2));

    let client = app.client().await;
    let count: i64 = client
        .query_one("select count(*) from posts", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, 2, "the rows are back");

    app.cleanup().await;
}

#[tokio::test]
async fn the_import_endpoint_refuses_an_option_it_does_not_have() {
    let Some(app) = test_app("backup_http_unknown_option").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let archive = export(&app, &token).await;

    // A client sending `deleteMissing` must be told the server has no such option,
    // rather than have it dropped while believing its intent was honoured.
    let reply = import(&app, &token, &archive, &[("deleteMissing", "true")]).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let message = reply.body.expect("an error")["message"]
        .as_str()
        .expect("a message")
        .to_string();
    assert!(message.contains("deleteMissing"), "{message}");
    assert!(message.contains("deletes nothing"), "{message}");

    app.cleanup().await;
}

#[tokio::test]
async fn the_preflight_reports_what_it_would_gate_on() {
    let Some(app) = test_app("backup_http_preflight").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/import/preflight",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a preflight");
    assert_eq!(body["available"], json!(true));
    assert_eq!(body["confirm_records"], json!(10_000));
    assert!(body["collections"].is_number(), "{body}");

    app.cleanup().await;
}

#[tokio::test]
async fn a_zip_that_is_not_a_lazypock_archive_is_refused_with_a_reason() {
    let Some(app) = test_app("backup_http_foreign").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = import(&app, &token, b"definitely not a zip", &[]).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let message = reply.body.expect("an error")["message"]
        .as_str()
        .expect("a message")
        .to_string();
    assert!(message.contains("archive"), "{message}");

    app.cleanup().await;
}
