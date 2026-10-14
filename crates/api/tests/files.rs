//! HTTP-level tests for file upload, serving and deletion.

mod support;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{test_app, TestApp};

/// A real, fully decodable PNG of the given dimensions.
///
/// It has to be a complete image, not just a header: upload validation reads the
/// dimensions out of the header without decoding, so a header-only fixture gets
/// stored happily and then fails at thumbnail time.
fn png(width: u32, height: u32) -> Vec<u8> {
    let raw = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 120, 200, 255]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(raw)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode png");
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
    support::superuser_token(app, "admin@example.com", "correct-horse").await
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

/// Upload a PNG and return its served URL.
async fn uploaded_png(app: &TestApp, token: &str, width: u32, height: u32) -> String {
    let bytes = png(width, height);
    let reply = upload(
        app,
        &[
            ("collection", None, b"posts"),
            ("record", None, b"p1"),
            ("file", Some("photo.png"), &bytes),
        ],
        Some(token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body.expect("body")["url"]
        .as_str()
        .expect("url")
        .to_string()
}

#[tokio::test]
async fn a_thumbnail_can_be_requested_by_size_preset_or_path() {
    let Some(app) = test_app("http_files_thumbnails").await else {
        return;
    };
    seed(&app).await;
    let token = token(&app).await;
    let source = png(400, 200);
    let url = uploaded_png(&app, &token, 400, 200).await;

    // An ad-hoc size fits inside the box, preserving the ratio.
    let (status, bytes) = fetch(&app, &format!("{url}?thumb=100x100")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body was: {}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(
        lazypock_files::sniff(&bytes),
        Some(lazypock_files::ImageFormat::Webp),
        "an ad-hoc size encodes to webp"
    );
    assert_eq!(
        lazypock_files::validation::dimensions(&bytes, lazypock_files::ImageFormat::Webp),
        Some((100, 50)),
        "the aspect ratio is preserved"
    );

    // Asking for a thumbnail must not disturb the original.
    let (status, original) = fetch(&app, &url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(original, source, "the original is served unchanged");

    // A stored preset, by query and by path.
    let (status, bytes) = fetch(&app, &format!("{url}?preset=thumb")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        lazypock_files::validation::dimensions(&bytes, lazypock_files::ImageFormat::Webp),
        Some((200, 200)),
        "the `thumb` preset is a 200x200 cover crop"
    );

    let (status, by_path) = fetch(&app, &format!("{url}/thumbs/thumb")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        by_path, bytes,
        "the path form and the query form return the same cached object"
    );

    // An unknown preset is a clear 404 rather than a silent fallback to the
    // original, which would look like success and serve the wrong size.
    let (status, _) = fetch(&app, &format!("{url}?preset=nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Asking for two sizes at once is ambiguous.
    let (status, _) = fetch(&app, &format!("{url}?thumb=10x10&preset=thumb")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    app.cleanup().await;
}

#[tokio::test]
async fn a_thumbnail_of_something_that_is_not_a_resizable_image_is_refused() {
    let Some(app) = test_app("http_files_thumbnail_non_image").await else {
        return;
    };
    seed(&app).await;
    let token = token(&app).await;

    // A GIF is a valid upload, so it stores fine, but we cannot produce a
    // thumbnail of it usefully and must say so rather than return something odd.
    let gif = b"GIF89a\x01\x00\x01\x00\x00\x00\x00;";
    let reply = upload(
        &app,
        &[
            ("collection", None, b"posts"),
            ("record", None, b"p1"),
            ("file", Some("anim.gif"), gif),
        ],
        Some(&token),
    )
    .await;
    if reply.status == StatusCode::CREATED {
        let url = reply.body.expect("body")["url"]
            .as_str()
            .expect("url")
            .to_string();
        let (status, body) = fetch(&app, &format!("{url}?thumb=32x32")).await;
        assert!(
            !status.is_server_error(),
            "an image we cannot resize must be a clear client error, not a 500:              {status} {}",
            String::from_utf8_lossy(&body)
        );
    }

    app.cleanup().await;
}
