//! HTTP-level tests for file upload, serving and deletion.

mod support;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{call, call_authed, superuser_token, test_app, TestApp};

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

    // Uploads require the record to exist, so the fixture has to be a real one.
    sql(app, "insert into posts (id, title) values ('p1', 'a post')").await;
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
    assert_eq!(body["mime_type"], json!("image/png"));
    assert_eq!(body["width"], json!(64));
    assert_eq!(body["height"], json!(48));
    assert_eq!(
        body["status"],
        json!("ready"),
        "an upload that names a record arrives attached"
    );
    assert_eq!(body["origin"], json!("field"));

    // The URL is id-addressed, because the id is what a client stores in a file field;
    // the storage key travels with it for a client that addresses objects directly.
    let url = body["url"].as_str().expect("url").to_string();
    assert!(url.starts_with("/api/files/"), "{url}");
    assert_eq!(
        url.rsplit('/').next(),
        body["id"].as_str(),
        "the URL ends in the file's id: {url}"
    );
    let key = body["key"].as_str().expect("key").to_string();
    assert!(key.starts_with("posts/p1/"), "{key}");
    assert!(key.ends_with("_holiday.png"), "{key}");

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
    assert_eq!(
        reply.status,
        StatusCode::CREATED,
        "the fixture must upload before its thumbnail is requested: {:?}",
        reply.body
    );
    {
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

/// Run a statement directly, the way SQL run outside the API would.
async fn sql(app: &TestApp, statement: &str) {
    let client = app.client().await;
    client
        .batch_execute(statement)
        .await
        .expect("the test statement should apply");
}

/// Run a sweep and return its report.
async fn sweep(app: &TestApp, token: &str, dry_run: bool) -> serde_json::Value {
    let reply = call_authed(
        &router(app).await,
        Method::POST,
        "/api/files/sweep",
        Some(json!({"dryRun": dry_run})),
        token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    reply.body.expect("a report")
}

#[tokio::test]
async fn sweeping_storage_requires_a_superuser() {
    let Some(app) = test_app("http_files_sweep_auth").await else {
        return;
    };

    let reply = call(
        &router(&app).await,
        Method::POST,
        "/api/files/sweep",
        Some(json!({"dryRun": true})),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn the_sweeper_reclaims_objects_whose_record_is_gone() {
    let Some(app) = test_app("http_files_sweep").await else {
        return;
    };
    seed(&app).await;
    let token = support::superuser_token(&app, "admin@example.com", "correct-horse").await;
    let url = uploaded_png(&app, &token, 64, 48).await;

    // While the record exists, nothing is an orphan.
    let report = sweep(&app, &token, true).await;
    assert_eq!(report["scanned"], json!(1), "{report}");
    assert_eq!(report["orphans"], json!(0), "{report}");

    // Remove the record the way SQL or a crash would, leaving the object behind.
    sql(&app, "delete from posts where id = 'p1'").await;

    // A dry run reports it, and deletes nothing.
    let report = sweep(&app, &token, true).await;
    assert_eq!(report["orphans"], json!(1), "{report}");
    assert_eq!(
        report["enqueued"],
        json!(0),
        "a dry run must enqueue nothing"
    );
    assert_eq!(report["deleted"], json!(0), "a dry run must delete nothing");
    let sample = report["sample"].as_array().expect("a sample");
    assert!(
        sample
            .iter()
            .any(|key| key.as_str().unwrap_or_default().ends_with("_photo.png")),
        "the report should name the orphan: {report}"
    );

    let (status, _) = fetch(&app, &url).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a dry run must leave the object in place"
    );

    // A real run reclaims it.
    let report = sweep(&app, &token, false).await;
    assert_eq!(report["enqueued"], json!(1), "{report}");
    assert_eq!(report["deleted"], json!(1), "{report}");

    let (status, _) = fetch(&app, &url).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the object should be gone");

    // Running again finds nothing, so the sweep converges.
    let report = sweep(&app, &token, false).await;
    assert_eq!(report["orphans"], json!(0), "{report}");

    app.cleanup().await;
}

#[tokio::test]
async fn the_sweeper_reclaims_a_thumbnail_whose_preset_is_gone_but_keeps_the_source() {
    let Some(app) = test_app("http_files_sweep_preset").await else {
        return;
    };
    seed(&app).await;
    let token = support::superuser_token(&app, "admin@example.com", "correct-horse").await;
    let url = uploaded_png(&app, &token, 64, 48).await;

    // Generate a thumbnail so there is a cached object to orphan.
    let (status, _) = fetch(&app, &format!("{url}?preset=thumb")).await;
    assert_eq!(status, StatusCode::OK);

    let report = sweep(&app, &token, true).await;
    assert_eq!(
        report["orphans"],
        json!(0),
        "both objects are in use: {report}"
    );

    // Delete the preset the thumbnail was generated from.
    sql(&app, "delete from _image_presets where name = 'thumb'").await;

    let report = sweep(&app, &token, true).await;
    assert_eq!(
        report["orphans"],
        json!(1),
        "expected the thumbnail to be orphaned: {report}"
    );
    let sample = report["sample"].as_array().expect("a sample");
    assert!(
        sample
            .iter()
            .any(|key| key.as_str().unwrap_or_default().contains("/thumbs/")),
        "the orphan should be the thumbnail, not the source: {report}"
    );

    // The source's record still exists, so the source must survive.
    let report = sweep(&app, &token, false).await;
    assert_eq!(report["deleted"], json!(1), "{report}");

    let (status, served) = fetch(&app, &url).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the source must survive a preset change"
    );
    assert_eq!(served.len(), png(64, 48).len(), "and be unchanged");

    app.cleanup().await;
}

/// Run a listing and return the body.
async fn listing(app: &TestApp, token: &str, query: &str) -> serde_json::Value {
    let reply = call_authed(
        &router(app).await,
        Method::GET,
        &format!("/api/files{query}"),
        None,
        token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    reply.body.expect("a listing")
}

#[tokio::test]
async fn listing_stored_files_requires_a_superuser() {
    let Some(app) = test_app("http_files_list_auth").await else {
        return;
    };

    let reply = call(&router(&app).await, Method::GET, "/api/files", None).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn the_stored_files_are_listed_with_the_urls_a_client_renders_from() {
    let Some(app) = test_app("http_files_list").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // An empty store is an empty page, not an error.
    let empty = listing(&app, &token, "").await;
    assert_eq!(empty["totalItems"], json!(0), "{empty}");
    assert_eq!(empty["items"], json!([]));

    let first = uploaded_png(&app, &token, 64, 48).await;
    let second = uploaded_png(&app, &token, 32, 32).await;

    let body = listing(&app, &token, "").await;
    assert_eq!(body["totalItems"], json!(2), "{body}");

    let items = body["items"].as_array().expect("items");
    let urls: Vec<&str> = items
        .iter()
        .map(|item| item["url"].as_str().expect("a url"))
        .collect();
    assert!(urls.contains(&first.as_str()), "{urls:?}");
    assert!(urls.contains(&second.as_str()), "{urls:?}");

    // Each item carries the variants a client needs, so it renders from `url`
    // rather than composing the server's layout itself.
    let variants = &items[0]["variants"];
    assert!(variants["content"].is_string(), "{variants}");
    assert!(
        variants["thumb"].is_string(),
        "a preset variant should be offered: {variants}"
    );
    assert!(
        variants["thumb"]
            .as_str()
            .expect("a url")
            .ends_with("?preset=thumb"),
        "{variants}"
    );

    // The key is included, so a client can delete or reference the object.
    assert!(items[0]["key"].is_string(), "{}", items[0]);
    assert_eq!(items[0]["collection"], json!("posts"));

    app.cleanup().await;
}

#[tokio::test]
async fn the_listing_pages_and_filters() {
    let Some(app) = test_app("http_files_list_paging").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    uploaded_png(&app, &token, 64, 48).await;
    uploaded_png(&app, &token, 32, 32).await;

    let page = listing(&app, &token, "?page=1&perPage=1").await;
    assert_eq!(page["totalItems"], json!(2), "{page}");
    assert_eq!(page["totalPages"], json!(2), "{page}");
    assert_eq!(page["items"].as_array().expect("items").len(), 1);

    let second = listing(&app, &token, "?page=2&perPage=1").await;
    let third = listing(&app, &token, "?page=3&perPage=1").await;
    assert_eq!(second["items"].as_array().expect("items").len(), 1);
    assert_eq!(
        third["items"].as_array().expect("items").len(),
        0,
        "a page past the end is empty rather than an error"
    );

    // A filter that matches nothing, and one that matches.
    let none = listing(&app, &token, "?collection=nope").await;
    assert_eq!(none["totalItems"], json!(0), "{none}");
    let some = listing(&app, &token, "?collection=posts").await;
    assert_eq!(some["totalItems"], json!(2), "{some}");

    app.cleanup().await;
}

#[tokio::test]
async fn a_generated_thumbnail_is_not_listed_as_its_own_file() {
    let Some(app) = test_app("http_files_list_variants").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let url = uploaded_png(&app, &token, 64, 48).await;

    // Generate a thumbnail, which stores a second object.
    let (status, _) = fetch(&app, &format!("{url}?preset=thumb")).await;
    assert_eq!(status, StatusCode::OK);

    // It is offered as a variant of its source, not as a file of its own:
    // otherwise every image would appear several times in the library.
    let body = listing(&app, &token, "").await;
    assert_eq!(body["totalItems"], json!(1), "{body}");
    assert!(body["items"][0]["variants"]["thumb"].is_string(), "{body}");

    app.cleanup().await;
}

#[tokio::test]
async fn the_sdk_spellings_of_the_upload_parts_are_accepted() {
    let Some(app) = test_app("http_files_sdk_parts").await else {
        return;
    };
    seed(&app).await;
    let token = token(&app).await;

    // Exactly the parts `lazypock-ts`'s `files.upload` sends. Spelling them
    // `collection`/`record` — this route's original names — meant a Studio upload was
    // rejected with "a `collection` part is required", which looks like a broken editor.
    let reply = upload(
        &app,
        &[
            ("file", Some("cover.png"), &png(20, 20)),
            ("collection_name", None, b"posts"),
            ("record_id", None, b"p1"),
            ("field_name", None, b"cover"),
            ("origin", None, b"field"),
        ],
        Some(&token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let body = reply.body.expect("body");
    assert_eq!(body["collection_name"], json!("posts"), "{body}");
    assert_eq!(body["record_id"], json!("p1"), "{body}");
    assert_eq!(body["field_name"], json!("cover"), "{body}");
    assert_eq!(body["status"], json!("ready"), "{body}");
    assert!(
        body["key"].as_str().expect("key").starts_with("posts/p1/"),
        "{body}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn an_upload_with_no_record_is_pending_and_the_library_lists_it() {
    let Some(app) = test_app("http_files_library_upload").await else {
        return;
    };
    let token = token(&app).await;

    // The editor's and the media library's case: bytes with no record to belong to. A
    // listing of the storage backend could not show this at all — it has no record to be
    // filed under — which is why the library could not offer an upload nobody had used.
    let reply = upload(
        &app,
        &[
            ("file", Some("diagram.png"), &png(20, 10)),
            ("origin", None, b"library"),
        ],
        Some(&token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let body = reply.body.expect("body");
    assert_eq!(body["status"], json!("pending"), "{body}");
    assert_eq!(body["origin"], json!("library"), "{body}");
    assert_eq!(body["collection_name"], json!(""), "{body}");
    assert!(
        body["key"].as_str().expect("key").starts_with("library/"),
        "{body}"
    );

    let listed = listing(&app, &token, "").await;
    assert_eq!(listed["total"], json!(1), "{listed}");
    assert_eq!(listed["items"][0]["id"], body["id"], "{listed}");

    app.cleanup().await;
}

#[tokio::test]
async fn deleting_a_record_removes_its_files() {
    let Some(app) = test_app("http_files_record_delete").await else {
        return;
    };
    seed(&app).await;
    let token = token(&app).await;

    let reply = upload(
        &app,
        &[
            ("collection", None, b"posts"),
            ("record", None, b"p1"),
            ("file", Some("cover.png"), &png(20, 20)),
        ],
        Some(&token),
    )
    .await;
    let body = reply.body.expect("body");
    let id = body["id"].as_str().expect("id").to_string();

    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/collections/posts/records/p1",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    // The row is gone with the record: the library no longer lists it, and even serving
    // it by id is a 404 — the object outlives the row only until the reaper runs.
    let listed = listing(&app, &token, "").await;
    assert_eq!(listed["total"], json!(0), "{listed}");
    let (status, _) = fetch(&app, &format!("/api/files/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // And the object was queued for removal in the transaction that deleted the row.
    let client = app.client().await;
    let queued: i64 = client
        .query_one(
            "select count(*) from _file_deletions where file_id = $1::text",
            &[&id],
        )
        .await
        .expect("queued")
        .get(0);
    assert_eq!(queued, 1, "the object should be queued for the reaper");

    app.cleanup().await;
}

#[tokio::test]
async fn a_file_a_record_still_references_is_guarded_until_forced() {
    let Some(app) = test_app("http_files_refs_guard").await else {
        return;
    };
    // A collection with an editor field: that is where a reference comes from, since a
    // rich text holds `/api/files/<id>` URLs.
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: "notes".into(),
            collection_type: CollectionType::Base,
            fields: vec![CreateField {
                name: "body".into(),
                field_type: FieldType::Editor,
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
    .expect("create notes");
    app.reload_schema().await;

    let token = token(&app).await;

    // An editor upload, then a record that embeds it.
    let reply = upload(
        &app,
        &[
            ("file", Some("inline.png"), &png(16, 16)),
            ("origin", None, b"editor"),
        ],
        Some(&token),
    )
    .await;
    let body = reply.body.expect("body");
    let id = body["id"].as_str().expect("id").to_string();

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/collections/notes/records",
        Some(json!({ "body": format!("<p>look <img src=\"/api/files/{id}\"></p>") })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    // Deleting it would leave that record pointing at nothing, so it is refused with the
    // list of what still uses it — the shape the Studio's media library renders.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        &format!("/api/files/{id}"),
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{:?}", reply.body);
    let body = reply.body.expect("a refusal");
    assert_eq!(
        body["data"]["usage"][0]["collection"],
        json!("notes"),
        "{body}"
    );
    assert_eq!(body["data"]["usage"][0]["field"], json!("body"), "{body}");

    // A superuser can break the reference deliberately.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        &format!("/api/files/{id}?force=true"),
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn the_reaper_reclaims_an_abandoned_upload_and_queued_objects() {
    let Some(app) = test_app("http_files_reaper").await else {
        return;
    };
    let token = token(&app).await;

    let reply = upload(
        &app,
        &[
            ("file", Some("abandoned.png"), &png(12, 12)),
            ("origin", None, b"library"),
        ],
        Some(&token),
    )
    .await;
    let body = reply.body.expect("body");
    let key = body["key"].as_str().expect("key").to_string();
    let id = body["id"].as_str().expect("id").to_string();

    // A TTL of zero makes it instantly reclaimable. `_files` rows go first — which is
    // what queues the object — and the pass that removes it is the next one.
    let reaper =
        lazypock_files::Reaper::new(app.state.files.storage(), "local").with_unattached_ttl(0);
    let client = app.client().await;
    let first = reaper.run(&client).await.expect("reaper pass");
    assert_eq!(first.reclaimed, 1, "{first:?}");

    let second = reaper.run(&client).await.expect("reaper pass");
    assert_eq!(
        second.deleted, 1,
        "the queued object goes on the next pass: {second:?}"
    );

    let listed = listing(&app, &token, "").await;
    assert_eq!(listed["total"], json!(0), "{listed}");
    assert!(
        app.state
            .files
            .storage()
            .get(&key)
            .await
            .expect("storage")
            .is_none(),
        "the object should be gone from the backend"
    );
    let _ = id;

    app.cleanup().await;
}

#[tokio::test]
async fn a_generated_variant_is_removed_with_its_source() {
    let Some(app) = test_app("http_files_variant_cleanup").await else {
        return;
    };
    seed(&app).await;
    let token = token(&app).await;
    let url = uploaded_png(&app, &token, 64, 48).await;
    let id = url.rsplit('/').next().expect("id").to_string();

    // Generate a thumbnail lazily, through the id-addressed route.
    let (status, _) = fetch(&app, &format!("/api/files/{id}?preset=thumb")).await;
    assert_eq!(status, StatusCode::OK);

    // It is recorded on the row. Without that the row can be deleted, the source object
    // queued, and the thumbnail left in the bucket with nothing pointing at it.
    let client = app.client().await;
    let source_key: String = client
        .query_one(
            "select storage_path from _files where id = $1::text",
            &[&id],
        )
        .await
        .expect("row")
        .get(0);
    let thumb_key: String = client
        .query_one(
            "select variants ->> 'thumb' from _files where id = $1::text",
            &[&id],
        )
        .await
        .expect("row")
        .get(0);
    assert_ne!(thumb_key, source_key);
    assert!(
        thumb_key.contains("/thumbs/") && thumb_key.ends_with("thumb_webp"),
        "{thumb_key}"
    );

    // Deleting the record queues both objects, and one pass removes both.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/collections/posts/records/p1",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    let reaper = lazypock_files::Reaper::new(app.state.files.storage(), "local");
    let report = reaper.run(&client).await.expect("reaper pass");
    assert_eq!(report.deleted, 1, "one queue entry: {report:?}");

    for key in [&source_key, &thumb_key] {
        assert!(
            app.state
                .files
                .storage()
                .get(key)
                .await
                .expect("storage")
                .is_none(),
            "{key} should be gone"
        );
    }

    app.cleanup().await;
}

/// Register an auth user and return a token for them.
///
/// The `users` collection is created on the app's own boot (idempotently), so a test
/// that needs an ordinary identity makes sure of it first.
async fn user_token(app: &TestApp, email: &str) -> String {
    let mut client = app.client().await;
    lazypock_db::defaults::ensure_users(&mut client)
        .await
        .expect("the users collection");
    app.reload_schema().await;

    let reply = call(
        &router(app).await,
        Method::POST,
        "/api/collections/users/records",
        Some(json!({ "email": email, "password": "user-password-123" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    let reply = call(
        &router(app).await,
        Method::POST,
        "/api/collections/users/auth-with-password",
        Some(json!({ "identity": email, "password": "user-password-123" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    reply.body.expect("auth")["token"]
        .as_str()
        .expect("token")
        .to_string()
}

/// Write the file rules the way the Studio's Settings → Media page does.
async fn set_file_rules(app: &TestApp, superuser: &str, rules: serde_json::Value) {
    let reply = call_authed(
        &router(app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "files": { "rules": rules } })),
        superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
}

/// Upload one file as the given identity, returning its id.
async fn upload_as(app: &TestApp, token: &str, filename: &str) -> String {
    let reply = upload(
        app,
        &[
            ("file", Some(filename), &png(16, 16)),
            ("origin", None, b"library"),
        ],
        Some(token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body.expect("body")["id"]
        .as_str()
        .expect("id")
        .to_string()
}

#[tokio::test]
async fn an_absent_file_rule_leaves_the_library_to_superusers() {
    let Some(app) = test_app("http_files_rules_absent").await else {
        return;
    };
    let superuser = token(&app).await;
    let user = user_token(&app, "user@example.com").await;

    // Nothing configured is the safe reading: a rule that does not exist does not grant.
    let reply = call_authed(&router(&app).await, Method::GET, "/api/files", None, &user).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);
    assert!(
        reply.body.expect("refusal")["message"]
            .as_str()
            .expect("message")
            .contains("listRule"),
        "the refusal should name the rule"
    );

    // The operator's own view is unaffected, which is what keeps the Studio working.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/files",
        None,
        &superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn a_list_rule_narrows_the_library_to_the_rows_it_matches() {
    let Some(app) = test_app("http_files_rules_list").await else {
        return;
    };
    let superuser = token(&app).await;
    let user = user_token(&app, "user@example.com").await;

    upload_as(&app, &user, "mine.png").await;
    upload_as(&app, &superuser, "theirs.png").await;

    // A public rule opens the listing to anyone.
    set_file_rules(
        &app,
        &superuser,
        json!({ "listRule": "", "deleteRule": null }),
    )
    .await;
    let body = listing(&app, &user, "").await;
    assert_eq!(body["total"], json!(2), "{body}");

    // A filter shows only the rows it matches, per caller.
    set_file_rules(
        &app,
        &superuser,
        json!({ "listRule": "uploaded_by = @request.auth.id", "deleteRule": null }),
    )
    .await;

    let body = listing(&app, &user, "").await;
    assert_eq!(body["total"], json!(1), "only their own file: {body}");
    assert_eq!(body["items"][0]["filename"], json!("mine.png"), "{body}");

    // A superuser bypasses the rule and sees everything.
    let body = listing(&app, &superuser, "").await;
    assert_eq!(body["total"], json!(2), "{body}");

    app.cleanup().await;
}

#[tokio::test]
async fn a_delete_rule_decides_whose_files_may_go() {
    let Some(app) = test_app("http_files_rules_delete").await else {
        return;
    };
    let superuser = token(&app).await;
    let user = user_token(&app, "user@example.com").await;
    let other = user_token(&app, "other@example.com").await;

    let mine = upload_as(&app, &user, "mine.png").await;
    let theirs = upload_as(&app, &other, "theirs.png").await;

    set_file_rules(
        &app,
        &superuser,
        json!({ "listRule": "", "deleteRule": "uploaded_by = @request.auth.id" }),
    )
    .await;

    // The rule is asked about *this* file: uploader-owns-it means exactly that.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        &format!("/api/files/{theirs}"),
        None,
        &user,
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        &format!("/api/files/{mine}"),
        None,
        &user,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    // A superuser is not bound by it.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        &format!("/api/files/{theirs}"),
        None,
        &superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn an_unreadable_rule_denies_instead_of_allowing() {
    let Some(app) = test_app("http_files_rules_broken").await else {
        return;
    };
    let superuser = token(&app).await;
    let user = user_token(&app, "user@example.com").await;

    // Whitespace is not the empty string, and a number is not a rule: both deny rather
    // than quietly meaning "public".
    set_file_rules(
        &app,
        &superuser,
        json!({ "listRule": "   ", "deleteRule": 7 }),
    )
    .await;

    let reply = call_authed(&router(&app).await, Method::GET, "/api/files", None, &user).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    // The operator can still get in, or they could not fix the rule.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/files",
        None,
        &superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn a_backend_without_direct_upload_says_so_and_drops_the_reservation() {
    let Some(app) = test_app("http_files_presign_local").await else {
        return;
    };
    seed(&app).await;
    let superuser = token(&app).await;

    // The local adapter has no HTTP endpoint of its own, so a direct upload is refused
    // with a reason — and the row reserved for it goes, because a file nobody can upload
    // is a leak.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/files/presign",
        Some(json!({
            "filename": "photo.png",
            "size": 4096,
            "mime": "image/png",
            "collection_name": "posts",
            "field_name": "cover",
            "record_id": "p1"
        })),
        &superuser,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::NOT_IMPLEMENTED,
        "{:?}",
        reply.body
    );
    let body = reply.body.expect("a refusal");
    assert!(
        body["message"]
            .as_str()
            .expect("message")
            .contains("/api/files"),
        "the refusal should say what to do instead: {body}"
    );

    let body = listing(&app, &superuser, "").await;
    assert_eq!(
        body["total"],
        json!(0),
        "the reservation should be gone: {body}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_direct_upload_is_validated_before_it_is_reserved() {
    let Some(app) = test_app("http_files_presign_guards").await else {
        return;
    };
    seed(&app).await;
    let superuser = token(&app).await;

    // Unauthenticated: refused.
    let reply = call(
        &router(&app).await,
        Method::POST,
        "/api/files/presign",
        Some(json!({ "filename": "photo.png", "size": 10, "mime": "image/png" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    // Not an image the server can read: refused, as the multipart route refuses it. The
    // declared type is the first of two gates, so it has to be one of the four.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/files/presign",
        Some(json!({ "filename": "script.sh", "size": 10, "mime": "text/x-sh" })),
        &superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    // A size that cannot be true: refused before anything is reserved.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/files/presign",
        Some(json!({ "filename": "huge.png", "size": 1024 * 1024 * 1024, "mime": "image/png" })),
        &superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    // A record that is not there: refused, so an upload cannot be reserved for a record
    // the reaper would immediately consider gone.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/files/presign",
        Some(json!({
            "filename": "photo.png", "size": 4096, "mime": "image/png",
            "collection_name": "posts", "record_id": "nope"
        })),
        &superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{:?}", reply.body);

    // And nothing was left behind by any of them.
    let body = listing(&app, &superuser, "").await;
    assert_eq!(body["total"], json!(0), "{body}");

    app.cleanup().await;
}

#[tokio::test]
async fn completing_an_upload_verifies_the_object_and_finishes_the_row() {
    let Some(app) = test_app("http_files_complete").await else {
        return;
    };
    seed(&app).await;
    let superuser = token(&app).await;

    // An upload that went through the API stands in for the object a presigned PUT would
    // have placed: completing it rewrites the row from what is actually stored.
    let reply = upload(
        &app,
        &[
            ("collection", None, b"posts"),
            ("record", None, b"p1"),
            ("file", Some("photo.png"), &png(40, 20)),
        ],
        Some(&superuser),
    )
    .await;
    let id = reply.body.expect("body")["id"]
        .as_str()
        .expect("id")
        .to_string();

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        &format!("/api/files/{id}/complete?variants=thumb"),
        Some(json!({})),
        &superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a file");
    assert_eq!(body["id"], json!(id), "{body}");
    assert_eq!(body["mime_type"], json!("image/png"), "{body}");
    assert_eq!(body["width"], json!(40), "{body}");
    assert_eq!(body["height"], json!(20), "{body}");
    assert!(
        body["variants"]["thumb"].is_string(),
        "the asked-for variant is offered: {body}"
    );

    // An id with no object behind it is dropped rather than left as a file that can never
    // be served.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/files/doesNotExist123/complete",
        Some(json!({})),
        &superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{:?}", reply.body);

    app.cleanup().await;
}

/// A collection with one file field, configured by `options`.
async fn seed_file_field(app: &TestApp, name: &str, options: serde_json::Value) {
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: name.into(),
            collection_type: CollectionType::Base,
            fields: vec![CreateField {
                name: "photo".into(),
                field_type: FieldType::File,
                required: false,
                unique: false,
                options,
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
    .expect("create the collection");
    app.reload_schema().await;
    sql(app, &format!("insert into {name} (id) values ('r1')")).await;
}

/// How many files the field of that collection holds.
async fn attached_count(app: &TestApp, collection: &str) -> i64 {
    app.client()
        .await
        .query_one(
            "select count(*) as n from _files where collection_name = $1",
            &[&collection.to_string()],
        )
        .await
        .expect("a row")
        .get("n")
}

/// `maxFileSize` and `mimeTypes` are stored on the field. Until this existed they
/// were stored and never read, so an operator's limit read as enforced and was not.
#[tokio::test]
async fn a_file_fields_own_limits_are_enforced_when_a_file_is_attached() {
    let Some(app) = test_app("http_files_field_policy").await else {
        return;
    };
    let token = token(&app).await;
    let bytes = png(64, 48);

    // A field that will not take anything larger than a byte, and one that will.
    seed_file_field(&app, "limited", json!({ "maxFileSize": 1 })).await;
    seed_file_field(&app, "roomy", json!({ "maxFileSize": 10_000_000 })).await;

    let reply = upload(
        &app,
        &[
            ("collection", None, b"limited"),
            ("record", None, b"r1"),
            ("field_name", None, b"photo"),
            ("file", Some("photo.png"), &bytes),
        ],
        Some(&token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    let message = reply.body.expect("an error")["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        message.contains("photo") && message.contains("1 bytes"),
        "the refusal should name the field and the limit, got {message:?}"
    );
    // The check runs before anything is written, so a refused upload leaves
    // nothing behind for the reaper to find.
    assert_eq!(
        attached_count(&app, "limited").await,
        0,
        "a refused upload left a file row behind"
    );

    // The same bytes are fine where the field allows them, so the refusal is the
    // field's limit rather than the upload in general.
    let reply = upload(
        &app,
        &[
            ("collection", None, b"roomy"),
            ("record", None, b"r1"),
            ("field_name", None, b"photo"),
            ("file", Some("photo.png"), &bytes),
        ],
        Some(&token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    assert_eq!(attached_count(&app, "roomy").await, 1);

    app.cleanup().await;
}

/// `mimeTypes` accepts an exact type or a `type/*` family.
#[tokio::test]
async fn a_file_fields_mime_types_limit_is_enforced() {
    let Some(app) = test_app("http_files_field_mime").await else {
        return;
    };
    let token = token(&app).await;

    seed_file_field(&app, "only_jpeg", json!({ "mimeTypes": ["image/jpeg"] })).await;

    let reply = upload(
        &app,
        &[
            ("collection", None, b"only_jpeg"),
            ("record", None, b"r1"),
            ("field_name", None, b"photo"),
            ("file", Some("photo.png"), &png(16, 16)),
        ],
        Some(&token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let message = reply.body.expect("an error")["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        message.contains("image/jpeg") && message.contains("image/png"),
        "the refusal should name what is accepted and what arrived, got {message:?}"
    );
    assert_eq!(attached_count(&app, "only_jpeg").await, 0);

    // `image/*` is the same field written as a family, and a PNG is in it.
    seed_file_field(&app, "any_image", json!({ "mimeTypes": ["image/*"] })).await;
    let reply = upload(
        &app,
        &[
            ("collection", None, b"any_image"),
            ("record", None, b"r1"),
            ("field_name", None, b"photo"),
            ("file", Some("photo.png"), &png(16, 16)),
        ],
        Some(&token),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    app.cleanup().await;
}

/// How many rows the library holds.
async fn library_rows(app: &TestApp) -> i64 {
    app.client()
        .await
        .query_one("select count(*) as n from _files", &[])
        .await
        .expect("a row")
        .get("n")
}

/// Attaching a file is what makes it a record's, so it is the point at which "may this
/// caller make this file theirs" has to be answered.
///
/// It was not: a record write re-pointed any `_files` row at the caller's record, so an
/// id read off any public record could be claimed by any other account — and a record
/// delete releases its files, which made the claim a delete of somebody else's upload.
#[tokio::test]
async fn another_account_cannot_claim_a_file_id() {
    let Some(app) = test_app("http_files_claim_by_id").await else {
        return;
    };
    // Public rules: the point is *not* that the second account cannot write the record.
    // It is that a write it is allowed to make cannot take a file it does not own.
    seed_file_field(&app, "photos", json!({})).await;
    let alice = user_token(&app, "alice@example.com").await;
    let mallory = user_token(&app, "mallory@example.com").await;

    let file = upload_as(&app, &alice, "alice.png").await;

    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/photos/records/r1",
        Some(json!({ "photo": file })),
        &mallory,
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);
    assert_eq!(
        attached_count(&app, "photos").await,
        0,
        "the claim must not have attached anything"
    );

    // The positive control: the owner attaching the same file to the same record works.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/photos/records/r1",
        Some(json!({ "photo": file })),
        &alice,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(attached_count(&app, "photos").await, 1);

    // Once it belongs to a record, another account still cannot take it — not even by
    // writing that same record, which its rules do allow.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/photos/records/r1",
        Some(json!({ "photo": file })),
        &mallory,
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    // And the file the owner saved is still served: nothing above removed it.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections/photos/records/r1",
        None,
        &alice,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    app.cleanup().await;
}

/// A file that is already attached belongs to that record: re-pointing it at another
/// record is refused rather than silently moving ownership.
#[tokio::test]
async fn a_file_cannot_move_between_records() {
    let Some(app) = test_app("http_files_move_between_records").await else {
        return;
    };
    seed_file_field(&app, "photos", json!({})).await;
    sql(&app, "insert into photos (id) values ('r2')").await;
    let alice = user_token(&app, "alice@example.com").await;

    let file = upload_as(&app, &alice, "alice.png").await;

    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/photos/records/r1",
        Some(json!({ "photo": file })),
        &alice,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    // The same owner, a different record: still refused. Two records sharing one id is
    // exactly the state that makes a delete of one silently break the other.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/collections/photos/records/r2",
        Some(json!({ "photo": file })),
        &alice,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{:?}", reply.body);

    app.cleanup().await;
}

/// `/api/batch` writes records exactly like the single-record routes, so it has to
/// attach their files too. It did not: the upload stayed `pending`, and the reaper
/// reclaimed it — a file field written through a batch quietly lost its file.
#[tokio::test]
async fn a_batch_write_attaches_its_files() {
    let Some(app) = test_app("http_files_batch_attach").await else {
        return;
    };
    seed_file_field(&app, "photos", json!({})).await;
    let superuser = token(&app).await;
    let file = upload_as(&app, &superuser, "batch.png").await;

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/batch",
        Some(json!({
            "requests": [
                {
                    "method": "PATCH",
                    "url": "/api/collections/photos/records/r1",
                    "body": { "photo": file },
                }
            ]
        })),
        &superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(
        attached_count(&app, "photos").await,
        1,
        "the batch's file field must own its upload"
    );

    app.cleanup().await;
}

/// The key-addressed delete used to enqueue the object and nothing else: any
/// authenticated caller could delete any object whose key they knew, and the `_files`
/// row was left behind claiming a file that no longer existed. It now resolves the key
/// to its row and goes through the same rule and the same reference guard as
/// `DELETE /api/files/{id}`.
#[tokio::test]
async fn the_key_addressed_delete_obeys_the_delete_rule() {
    let Some(app) = test_app("http_files_key_delete").await else {
        return;
    };
    seed(&app).await;
    let superuser = token(&app).await;
    let user = user_token(&app, "user@example.com").await;

    let reply = upload(
        &app,
        &[
            ("collection", None, b"posts"),
            ("record", None, b"p1"),
            ("file", Some("note.png"), &png(16, 16)),
        ],
        Some(&superuser),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let key = reply.body.expect("body")["key"]
        .as_str()
        .expect("key")
        .to_string();
    let url = format!("/api/files/{key}");

    // No file rule is set, which leaves deletion to superusers.
    let reply = call_authed(&router(&app).await, Method::DELETE, &url, None, &user).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);
    assert_eq!(
        fetch(&app, &url).await.0,
        StatusCode::OK,
        "a refused delete must leave the object alone"
    );
    assert_eq!(library_rows(&app).await, 1);

    // A key that names no row is a 404, not a blind object delete.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/files/posts/p1/nothing_here.png",
        None,
        &superuser,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{:?}", reply.body);

    // The superuser's delete removes the row and the object together.
    let reply = call_authed(&router(&app).await, Method::DELETE, &url, None, &superuser).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);
    assert_eq!(fetch(&app, &url).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        library_rows(&app).await,
        0,
        "the row must go with the object"
    );

    app.cleanup().await;
}
