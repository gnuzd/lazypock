//! Tests for the database-free routes, driven in-process.

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use lazypock_api::router::health_router;
use serde_json::Value;
use tower::ServiceExt;

async fn get(uri: &str) -> (StatusCode, Value) {
    let response = health_router()
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

fn router() -> Router {
    health_router()
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
async fn health_reports_the_running_version() {
    let (status, body) = get("/api/health").await;
    assert_eq!(status, StatusCode::OK);

    // The build that answers is the build that reports: a deployment can tell a
    // stale container from a fresh one from the probe alone.
    let version = body["data"]["version"]
        .as_str()
        .expect("`data.version` must be a string");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert!(
        version.split('.').count() >= 3,
        "`{version}` does not look like a release version"
    );
}

#[tokio::test]
async fn unknown_route_returns_the_error_envelope() {
    let (status, body) = get("/api/does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["status"], 404);
    assert_eq!(body["message"], "The requested resource was not found.");
    assert!(body["data"].is_object());
}

#[tokio::test]
async fn the_health_router_has_no_state() {
    // Constructing it needs no database, which keeps these tests hermetic.
    let _: Router = router();
}
