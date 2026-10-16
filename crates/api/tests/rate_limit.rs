//! Per-node request rate limiting.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;

use serde_json::json;

use support::{call, test_app_with, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// A public route, so a request spends its allowance without needing a token.
const PUBLIC: &str = "/api/jwks";

#[tokio::test]
async fn a_client_that_has_spent_its_allowance_is_refused() {
    let Some(app) = test_app_with("rate_limit_spent", &[("LAZYPOCK_RATE_LIMIT_API", "2")]).await
    else {
        return;
    };
    let router = router(&app).await;

    for attempt in 1..=2 {
        let reply = call(&router, Method::GET, PUBLIC, None).await;
        assert_eq!(reply.status, StatusCode::OK, "request {attempt}");
    }

    let reply = call(&router, Method::GET, PUBLIC, None).await;
    assert_eq!(
        reply.status,
        StatusCode::TOO_MANY_REQUESTS,
        "the third request should have been refused"
    );
    let message = reply.body.expect("a body")["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        message.contains("Too many requests"),
        "the refusal should say so, got {message:?}"
    );

    app.cleanup().await;
}

/// A load balancer polls the health probe on its own schedule; refusing one would
/// take the node out of rotation for being polled.
#[tokio::test]
async fn the_health_probe_is_never_limited() {
    let Some(app) = test_app_with("rate_limit_health", &[("LAZYPOCK_RATE_LIMIT_API", "1")]).await
    else {
        return;
    };
    let router = router(&app).await;

    for _ in 0..5 {
        let reply = call(&router, Method::GET, "/api/health", None).await;
        assert_eq!(reply.status, StatusCode::OK);
    }

    app.cleanup().await;
}

/// Signing in gets a much smaller allowance than an ordinary read, because a
/// person signs in a handful of times and a password being guessed does it
/// thousands.
#[tokio::test]
async fn credentials_have_their_own_smaller_allowance() {
    let Some(app) = test_app_with(
        "rate_limit_credentials",
        &[
            ("LAZYPOCK_RATE_LIMIT_API", "100"),
            ("LAZYPOCK_RATE_LIMIT_AUTH", "1"),
        ],
    )
    .await
    else {
        return;
    };
    let router = router(&app).await;

    // A failed sign-in counts: that is what an attempt to guess looks like from
    // here, and it must not be free.
    let body = json!({ "identity": "nobody@example.com", "password": "wrong" });
    let first = call(
        &router,
        Method::POST,
        "/api/collections/_superusers/auth-with-password",
        Some(body.clone()),
    )
    .await;
    assert_ne!(
        first.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{:?}",
        first.body
    );

    let second = call(
        &router,
        Method::POST,
        "/api/collections/_superusers/auth-with-password",
        Some(body),
    )
    .await;
    assert_eq!(
        second.status,
        StatusCode::TOO_MANY_REQUESTS,
        "the second sign-in should have been refused"
    );

    // And the ordinary allowance is untouched by it.
    let reply = call(&router, Method::GET, PUBLIC, None).await;
    assert_eq!(reply.status, StatusCode::OK);

    app.cleanup().await;
}

#[tokio::test]
async fn a_zero_limit_disables_limiting() {
    let Some(app) = test_app_with("rate_limit_off", &[("LAZYPOCK_RATE_LIMIT_API", "0")]).await
    else {
        return;
    };
    let router = router(&app).await;

    for attempt in 1..=10 {
        let reply = call(&router, Method::GET, PUBLIC, None).await;
        assert_eq!(reply.status, StatusCode::OK, "request {attempt}");
    }

    app.cleanup().await;
}
