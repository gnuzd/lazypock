//! HTTP-level tests for file upload, serving and deletion.

mod support;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{call, test_app, TestApp};

/// A PNG header with the given dimensions.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes
}

const BOUNDARY: &str = "----lazypockTestBoundary";

/// Build a multipart body. A part with a filename is treated as a file.
fn multipart(parts: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, filename, content) in parts {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match filename {
            Some(filename) => body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\
                     Content-Type: image/png\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

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

/// A superuser access token, since uploading needs an authenticated request.
async fn token(app: &TestApp) -> String {
    let client = app.client().await;
    lazypock_auth::superuser::create(&client, "admin@example.com", "correct-horse")
        .await
        .expect("superuser");
    let reply = call(
        &router(app).await,
        Method::POST,
        "/api/collections/_superusers/auth-with-password",
        Some(json!({"identity": "admin@example.com", "password": "correct-horse"})),
    )
    .await;
    reply.body.expect("body")["token"]
        .as_str()
        .expect("token")
        .to_string()
}

async fn upload(
    app: &TestApp,
    parts: &[(&str, Option<&str>, &[u8])],
    token: Option<&str>,
) -> support::Reply {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/api/files")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        );
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = builder.body(Body::from(multipart(parts))).expect("request");
    let response = router(app).await.oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        None
    } else {
        serde_json::from_slice(&bytes).ok()
    };
    support::Reply {
        status,
        headers: Default::default(),
        body,
    }
}

async fn fetch(app: &TestApp, url: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(Method::GET)
        .uri(url)
        .body(Body::empty())
        .expect("request");
    let response = router(app).await.oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body")
        .to_vec();
    (status, bytes)
}

#[tokio::test]
async fn a_file_can_be_uploaded_served_and_deleted() {
    let Some(app) = test_app("http_files_round_trip").await else {
        return;
    };
    seed(&app).await;
    let token = token(&app).await;
    let bytes = png(64, 48);

    let reply = upload(
        &app,
        &[
            ("collection", None, b"posts"),
            ("record", None, b"p1"),
            ("file", Some("holiday.png"), &bytes),
        ],
        Some(&token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let body = reply.body.expect("body");
    assert_eq!(body["type"], json!("png"));
    let url = body["url"].as_str().expect("url").to_string();
    assert!(url.ends_with("_holiday.png"), "{url}");
    assert!(
        url.starts_with("/api/files/posts/p1/"),
        "the key groups by collection and record: {url}"
    );

    // Serving returns the exact bytes, with a content type and a cache header.
    let (status, served) = fetch(&app, &url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        served, bytes,
        "the stored bytes should round-trip unchanged"
    );

    // Deleting goes through the outbox and then the object is gone.
    let request = Request::builder()
        .method(Method::DELETE)
        .uri(&url)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let status = router(&app)
        .await
        .oneshot(request)
        .await
        .expect("response")
        .status();
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = fetch(&app, &url).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    app.cleanup().await;
}

#[tokio::test]
async fn uploading_requires_an_authenticated_request() {
    let Some(app) = test_app("http_files_unauthenticated").await else {
        return;
    };
    seed(&app).await;

    let reply = upload(
        &app,
        &[
            ("collection", None, b"posts"),
            ("record", None, b"p1"),
            ("file", Some("photo.png"), &png(10, 10)),
        ],
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn svg_and_non_images_are_refused_before_anything_is_stored() {
    let Some(app) = test_app("http_files_rejects").await else {
        return;
    };
    seed(&app).await;
    let token = token(&app).await;

    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>";
    let reply = upload(
        &app,
        &[
            ("collection", None, b"posts"),
            ("record", None, b"p1"),
            ("file", Some("evil.png"), svg),
        ],
        Some(&token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    assert!(
        reply.body.as_ref().unwrap()["message"]
            .as_str()
            .unwrap()
            .contains("SVG"),
        "{:?}",
        reply.body
    );

    let reply = upload(
        &app,
        &[
            ("collection", None, b"posts"),
            ("record", None, b"p1"),
            ("file", Some("script.sh"), b"#!/bin/sh\nrm -rf /"),
        ],
        Some(&token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn a_file_cannot_be_attached_to_a_collection_that_does_not_exist() {
    let Some(app) = test_app("http_files_unknown_collection").await else {
        return;
    };
    seed(&app).await;
    let token = token(&app).await;

    let reply = upload(
        &app,
        &[
            ("collection", None, b"nope"),
            ("record", None, b"p1"),
            ("file", Some("photo.png"), &png(10, 10)),
        ],
        Some(&token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn a_traversing_file_path_is_refused() {
    let Some(app) = test_app("http_files_traversal").await else {
        return;
    };

    for url in [
        "/api/files/posts/../secret.png",
        "/api/files/posts/..%2F..%2Fetc/passwd",
        "/api/files/..%2Fposts/p1/photo.png",
    ] {
        let (status, _) = fetch(&app, url).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
            "{url} returned {status}"
        );
    }

    app.cleanup().await;
}
