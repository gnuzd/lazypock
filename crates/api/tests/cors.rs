//! Cross-origin requests, driven by the `cors_origins` setting.

mod support;

use axum::body::Body;
use axum::http::{header, Method, Request, Response, StatusCode};
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// Send a request with an `Origin` header and return the response.
async fn with_origin(app: &TestApp, method: Method, uri: &str, origin: &str) -> Response<Body> {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::ORIGIN, origin)
        .body(Body::empty())
        .expect("request");
    router(app).await.oneshot(request).await.expect("response")
}

/// What the CORS headers say, if anything.
fn allow_origin(response: &Response<Body>) -> Option<String> {
    response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

#[tokio::test]
async fn only_a_configured_origin_is_allowed() {
    let Some(app) = test_app("cors_allowlist").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Nothing configured: no origin is allowed.
    let response = with_origin(&app, Method::GET, "/api/health", "https://app.example").await;
    assert_eq!(
        allow_origin(&response),
        None,
        "an unconfigured installation must allow nothing"
    );
    assert!(
        response
            .headers()
            .get(header::VARY)
            .is_some_and(|value| value.to_str().unwrap_or_default().contains("Origin")),
        "the response differs by origin, so a shared cache must be told"
    );

    // Allow one.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "cors_origins": ["https://app.example"] })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    // The refresh is what applies it without a restart.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/settings/refresh-cors",
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(
        reply.body.expect("origins")["cors_origins"],
        json!(["https://app.example"])
    );

    // The allowed origin is echoed — not `*`, which is invalid with credentials.
    let response = with_origin(&app, Method::GET, "/api/health", "https://app.example").await;
    assert_eq!(
        allow_origin(&response).as_deref(),
        Some("https://app.example")
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );

    // A different origin is not, and a trailing slash does not change that either
    // way: origins are compared without one.
    let response = with_origin(&app, Method::GET, "/api/health", "https://evil.example").await;
    assert_eq!(allow_origin(&response), None);
    // The echo is the origin *as sent*, which is what the spec requires — a browser
    // compares the header exactly, so normalising it here would break the match. A
    // browser never sends a trailing slash, so this is about the comparison rather
    // than the echo.
    let response = with_origin(&app, Method::GET, "/api/health", "https://app.example/").await;
    assert_eq!(
        allow_origin(&response).as_deref(),
        Some("https://app.example/"),
        "the origin is echoed verbatim, as the browser will compare it"
    );

    // The normalisation that matters is against the *configured* list: an entry
    // saved with a trailing slash must still match the origin a browser sends.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "cors_origins": ["https://saved.example/"] })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    call_authed(
        &router(&app).await,
        Method::POST,
        "/api/settings/refresh-cors",
        Some(json!({})),
        &token,
    )
    .await;

    let response = with_origin(&app, Method::GET, "/api/health", "https://saved.example").await;
    assert_eq!(
        allow_origin(&response).as_deref(),
        Some("https://saved.example"),
        "a configured trailing slash must not make the origin unreachable"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_preflight_is_answered_for_an_allowed_origin() {
    let Some(app) = test_app("cors_preflight").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "cors_origins": ["*"] })),
        &token,
    )
    .await;
    call_authed(
        &router(&app).await,
        Method::POST,
        "/api/settings/refresh-cors",
        Some(json!({})),
        &token,
    )
    .await;

    // A preflight names a route that does not accept OPTIONS, so it is answered by
    // the CORS layer rather than the router.
    let request = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/collections/posts/records")
        .header(header::ORIGIN, "https://app.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .expect("request");
    let response = router(&app).await.oneshot(request).await.expect("response");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        allow_origin(&response).as_deref(),
        Some("https://app.example")
    );
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .is_some_and(|value| value.to_str().unwrap_or_default().contains("POST")),
        "the methods must be advertised"
    );
    // The headers the client asked for are the ones echoed back.
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .and_then(|v| v.to_str().ok()),
        Some("content-type")
    );

    app.cleanup().await;
}

#[tokio::test]
async fn refreshing_the_allowlist_requires_a_superuser() {
    let Some(app) = test_app("cors_refresh_auth").await else {
        return;
    };

    let reply = call(
        &router(&app).await,
        Method::POST,
        "/api/settings/refresh-cors",
        Some(json!({})),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    app.cleanup().await;
}
