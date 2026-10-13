//! End-to-end tests for the HTTP surface, driving the real router in-process.

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use lazypock_api::router::{router, AppState};
use lazypock_core::config::Config;
use serde_json::Value;
use tower::ServiceExt;

fn test_config() -> Config {
    Config::from_source(|key| match key {
        "DATABASE_URL" => Some("postgres://user:pass@localhost:5432/lazypock".to_string()),
        _ => None,
    })
    .expect("test configuration should be valid")
}

fn app() -> Router {
    router(AppState::new(test_config()))
}

async fn get(uri: &str) -> (StatusCode, Value) {
    let response = app()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("router should respond");

    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

#[tokio::test]
async fn health_returns_the_pocketbase_envelope() {
    let (status, body) = get("/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 200);
    assert_eq!(body["message"], "OK");
    assert!(body["data"].is_object());
}

#[tokio::test]
async fn unknown_route_returns_the_error_envelope() {
    let (status, body) = get("/api/does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["status"], 404);
    assert_eq!(body["message"], "The requested resource was not found.");
    assert!(body["data"].is_object());
}
