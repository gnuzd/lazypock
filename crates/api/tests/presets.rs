//! HTTP tests for image preset administration.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::json;

use support::{call, call_authed, get, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

#[tokio::test]
async fn the_seeded_presets_are_listed_without_a_token() {
    let Some(app) = test_app("http_presets_list").await else {
        return;
    };

    let reply = get(&router(&app).await, "/api/image-presets").await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let body = reply.body.expect("a body");
    let names: Vec<&str> = body
        .as_array()
        .expect("an array")
        .iter()
        .map(|preset| preset["name"].as_str().expect("a name"))
        .collect();
    for expected in ["thumb", "sm", "md", "lg"] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }

    // The shape a client needs is all present.
    let thumb = body
        .as_array()
        .expect("an array")
        .iter()
        .find(|preset| preset["name"] == json!("thumb"))
        .expect("the thumb preset");
    assert_eq!(thumb["width"], json!(200));
    assert_eq!(thumb["height"], json!(200));
    assert_eq!(thumb["fit"], json!("cover"));
    assert_eq!(thumb["format"], json!("webp"));

    app.cleanup().await;
}

#[tokio::test]
async fn writing_a_preset_requires_a_superuser() {
    let Some(app) = test_app("http_presets_auth").await else {
        return;
    };
    let created =
        json!({"name": "card", "width": 400, "height": 300, "fit": "inside", "format": "webp"});

    // Anonymous.
    let reply = call(
        &router(&app).await,
        Method::POST,
        "/api/image-presets",
        Some(created.clone()),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    // ... and deleting without a token.
    let reply = call(
        &router(&app).await,
        Method::DELETE,
        "/api/image-presets/thumb",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    // The refusal must not have changed anything.
    let reply = get(&router(&app).await, "/api/image-presets").await;
    let names: Vec<String> = reply
        .body
        .expect("a body")
        .as_array()
        .expect("an array")
        .iter()
        .map(|preset| preset["name"].as_str().expect("a name").to_string())
        .collect();
    assert!(names.contains(&"thumb".to_string()), "{names:?}");
    assert!(!names.contains(&"card".to_string()), "{names:?}");

    app.cleanup().await;
}

#[tokio::test]
async fn a_preset_can_be_created_updated_and_deleted() {
    let Some(app) = test_app("http_presets_crud").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Create.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/image-presets",
        Some(
            json!({"name": "card", "width": 400, "height": 300, "fit": "inside", "format": "webp"}),
        ),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    // A second one with the same name is a conflict, not a silent overwrite.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/image-presets",
        Some(json!({"name": "card", "width": 1, "height": 1, "fit": "inside", "format": "webp"})),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{:?}", reply.body);

    // A thumbnail can immediately be requested at the new size.
    let reply = get(&router(&app).await, "/api/image-presets").await;
    let card = reply
        .body
        .expect("a body")
        .as_array()
        .expect("an array")
        .iter()
        .find(|preset| preset["name"] == json!("card"))
        .cloned()
        .expect("the card preset");
    assert_eq!(card["width"], json!(400));
    assert_eq!(card["quality"], json!(80), "quality defaults to 80");

    // Update.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/image-presets/card",
        Some(json!({"width": 640, "height": 480, "fit": "cover", "format": "jpeg", "quality": 70})),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("a body")["quality"], json!(70));

    let reply = get(&router(&app).await, "/api/image-presets").await;
    let card = reply
        .body
        .expect("a body")
        .as_array()
        .expect("an array")
        .iter()
        .find(|preset| preset["name"] == json!("card"))
        .cloned()
        .expect("the card preset");
    assert_eq!(card["width"], json!(640));
    assert_eq!(card["fit"], json!("cover"));
    assert_eq!(card["format"], json!("jpeg"));

    // Updating something that does not exist is a 404, not a silent create.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/image-presets/nope",
        Some(json!({"width": 10, "height": 10, "fit": "inside", "format": "webp"})),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{:?}", reply.body);

    // Delete.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/image-presets/card",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/image-presets/card",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn an_invalid_preset_is_refused_with_a_reason() {
    let Some(app) = test_app("http_presets_invalid").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let cases = [
        // A fit mode the schema's check constraint does not allow.
        (
            json!({"name": "a", "width": 10, "height": 10, "fit": "squash", "format": "webp"}),
            "fit",
        ),
        // A format the schema does not allow.
        (
            json!({"name": "b", "width": 10, "height": 10, "fit": "inside", "format": "bmp"}),
            "format",
        ),
        // AVIF is allowed by the schema but not implemented, so it is refused
        // when configured rather than on every request.
        (
            json!({"name": "c", "width": 10, "height": 10, "fit": "inside", "format": "avif"}),
            "AVIF",
        ),
        // A name that would escape the key it becomes part of.
        (
            json!({"name": "../etc", "width": 10, "height": 10, "fit": "inside", "format": "webp"}),
            "name",
        ),
        // No dimensions at all.
        (
            json!({"name": "d", "fit": "inside", "format": "webp"}),
            "width",
        ),
        // Out of range.
        (
            json!({"name": "e", "width": 0, "height": 10, "fit": "inside", "format": "webp"}),
            "width",
        ),
        (
            json!({"name": "f", "width": 10, "height": 10, "fit": "inside", "format": "webp", "quality": 0}),
            "quality",
        ),
        // A missing name.
        (
            json!({"width": 10, "height": 10, "fit": "inside", "format": "webp"}),
            "name",
        ),
    ];

    for (body, expected) in cases {
        let reply = call_authed(
            &router(&app).await,
            Method::POST,
            "/api/image-presets",
            Some(body.clone()),
            &token,
        )
        .await;

        assert!(
            reply.status.is_client_error() || reply.status == StatusCode::NOT_IMPLEMENTED,
            "{body} should be refused, got {} {:?}",
            reply.status,
            reply.body
        );
        let message = reply.body.as_ref().expect("a body")["message"]
            .as_str()
            .expect("a message")
            .to_string();
        assert!(
            message.to_lowercase().contains(&expected.to_lowercase()),
            "{body} should mention {expected:?}, said {message:?}"
        );
    }

    // None of those attempts should have created anything.
    let reply = get(&router(&app).await, "/api/image-presets").await;
    let body = reply.body.expect("a body");
    let names: Vec<&str> = body
        .as_array()
        .expect("an array")
        .iter()
        .map(|preset| preset["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(
        names.len(),
        4,
        "no preset should have been created: {names:?}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn renaming_a_preset_is_refused_rather_than_silently_ignored() {
    let Some(app) = test_app("http_presets_rename").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // The body naming something other than the path is a mistake worth reporting:
    // honouring it would rename the row and orphan every cached thumbnail.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/image-presets/thumb",
        Some(
            json!({"name": "tile", "width": 100, "height": 100, "fit": "cover", "format": "webp"}),
        ),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    // The original still has its own size.
    let reply = get(&router(&app).await, "/api/image-presets").await;
    let thumb = reply
        .body
        .clone()
        .expect("a body")
        .as_array()
        .expect("an array")
        .iter()
        .find(|preset| preset["name"] == json!("thumb"))
        .cloned()
        .expect("the thumb preset");
    assert_eq!(thumb["width"], json!(200));
    let renamed: Vec<&str> = reply
        .body
        .as_ref()
        .and_then(|body| body.as_array())
        .expect("an array")
        .iter()
        .map(|preset| preset["name"].as_str().expect("a name"))
        .collect();
    assert!(!renamed.contains(&"tile"), "{renamed:?}");

    app.cleanup().await;
}
