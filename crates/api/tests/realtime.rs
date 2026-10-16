//! The realtime SSE endpoint's subscription contract.
//!
//! The per-record fan-out is covered in `lazypock-realtime`'s own tests; this checks
//! the endpoint-level rules: the collection **registry** topic is accepted, but only
//! for a superuser, and it opens a stream.

mod support;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use tower::ServiceExt;

use support::{superuser_token, test_app};

#[tokio::test]
async fn the_registry_topic_is_superuser_only() {
    let Some(app) = test_app("realtime_registry").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let router = lazypock_api::router::router(app.state.clone());

    // Anonymous: refused before a stream is opened. The registry payload is a
    // collection's definition, which carries its access rules.
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/realtime?subscribe=collections")
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // A superuser gets a stream. The body is not read: the response is returned as
    // soon as the handler builds the (lazy) stream, which is all this asserts.
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/realtime?subscribe=collections")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let response = router.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        content_type.starts_with("text/event-stream"),
        "{content_type}"
    );

    app.cleanup().await;
}
