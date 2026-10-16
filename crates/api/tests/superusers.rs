//! First-run setup: the check, and creating the first superuser.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

use support::{call, get, post, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// How many superusers exist, counted directly.
async fn superuser_count(app: &TestApp) -> i64 {
    let client = app.client().await;
    client
        .query_one("select count(*) from _superusers", &[])
        .await
        .expect("count")
        .get(0)
}

#[tokio::test]
async fn a_fresh_installation_reports_that_it_needs_setup() {
    let Some(app) = test_app("setup_check_fresh").await else {
        return;
    };

    let reply = get(&router(&app).await, "/api/superusers/check").await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(
        reply.body.expect("a check")["has_superuser"],
        json!(false),
        "a fresh installation has no superuser"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn setup_creates_the_first_superuser_and_logs_it_in() {
    let Some(app) = test_app("setup_first").await else {
        return;
    };

    let reply = post(
        &router(&app).await,
        "/api/superusers/setup",
        json!({ "email": "admin@example.com", "password": "correct-horse" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    let body = reply.body.expect("a setup");
    let token = body["token"].as_str().expect("a token").to_string();
    assert_eq!(body["record"]["email"], json!("admin@example.com"));
    assert_eq!(body["record"]["collectionName"], json!("_superusers"));

    // The installation now reports as set up...
    let reply = get(&router(&app).await, "/api/superusers/check").await;
    assert_eq!(reply.body.expect("a check")["has_superuser"], json!(true));

    // ... and the token works, so setup cannot hand back something login would
    // refuse.
    let request = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/api/me")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(axum::body::Body::empty())
        .expect("request");
    let response = router(&app).await.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    assert_eq!(superuser_count(&app).await, 1);

    app.cleanup().await;
}

#[tokio::test]
async fn setup_is_refused_once_a_superuser_exists() {
    // The guard that makes a public setup endpoint safe. Without it, anyone able to
    // reach the endpoint could make themselves a superuser on a running
    // installation, which is a complete takeover.
    let Some(app) = test_app("setup_takeover").await else {
        return;
    };

    let reply = post(
        &router(&app).await,
        "/api/superusers/setup",
        json!({ "email": "owner@example.com", "password": "correct-horse" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    // An attacker tries to add a second account.
    let reply = post(
        &router(&app).await,
        "/api/superusers/setup",
        json!({ "email": "attacker@example.com", "password": "hunter2-hunter2" }),
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::FORBIDDEN,
        "a second setup must be refused: {:?}",
        reply.body
    );

    // Nothing was created, and the original account still works.
    assert_eq!(superuser_count(&app).await, 1);
    let client = app.client().await;
    let attacker = client
        .query_opt(
            "select 1 from _superusers where email = 'attacker@example.com'",
            &[],
        )
        .await
        .expect("query");
    assert!(attacker.is_none(), "the attacker's account must not exist");

    app.cleanup().await;
}

#[tokio::test]
async fn two_concurrent_setups_cannot_both_win() {
    // The race the advisory lock exists for: on a fresh installation, two requests
    // arriving together must not both see zero superusers and create one each.
    let Some(app) = test_app("setup_race").await else {
        return;
    };

    let router = router(&app).await;
    let first = post(
        &router,
        "/api/superusers/setup",
        json!({ "email": "one@example.com", "password": "correct-horse" }),
    );
    let second = post(
        &router,
        "/api/superusers/setup",
        json!({ "email": "two@example.com", "password": "correct-horse" }),
    );
    let (first, second) = tokio::join!(first, second);

    let created = [&first, &second]
        .iter()
        .filter(|reply| reply.status == StatusCode::CREATED)
        .count();
    assert_eq!(
        created, 1,
        "exactly one must win, got {} and {}",
        first.status, second.status
    );
    assert_eq!(
        superuser_count(&app).await,
        1,
        "exactly one superuser must exist"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn setup_validates_what_it_is_given() {
    let Some(app) = test_app("setup_validation").await else {
        return;
    };
    let router = router(&app).await;

    let cases = [
        (
            json!({ "email": "admin@example.com", "password": "short" }),
            "password",
        ),
        (
            json!({ "email": "not-an-email", "password": "correct-horse" }),
            "email",
        ),
        (json!({ "email": "", "password": "correct-horse" }), "email"),
        (
            // A typo'd field is refused rather than ignored, so a client cannot
            // believe it set something it did not.
            json!({ "email": "admin@example.com", "passwrd": "correct-horse" }),
            "",
        ),
    ];

    for (body, expected) in cases {
        let reply = post(&router, "/api/superusers/setup", body.clone()).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{body} should be refused: {:?}",
            reply.body
        );
        if !expected.is_empty() {
            let message = reply.body.expect("an error")["message"]
                .as_str()
                .expect("a message")
                .to_string();
            assert!(message.contains(expected), "{body} -> {message}");
        }
    }

    // Nothing was created by any of them.
    assert_eq!(superuser_count(&app).await, 0);

    app.cleanup().await;
}

#[tokio::test]
async fn the_setup_endpoints_need_no_token() {
    // They cannot: a client calls them before anybody can log in. Asserted so a
    // later change that adds authentication here is caught as a regression rather
    // than discovered by a user staring at a login screen with no way past it.
    let Some(app) = test_app("setup_public").await else {
        return;
    };
    let router = router(&app).await;

    let reply = call(&router, Method::GET, "/api/superusers/check", None).await;
    let status = reply.status;
    assert!(
        status != StatusCode::UNAUTHORIZED,
        "the endpoint must be reachable without a token, got {status}"
    );
    let status = reply.status;
    assert!(
        status != StatusCode::FORBIDDEN,
        "the endpoint must be reachable without a token, got {status}"
    );

    let reply = post(
        &router,
        "/api/superusers/setup",
        json!({ "email": "admin@example.com", "password": "correct-horse" }),
    )
    .await;
    let status = reply.status;
    assert!(
        status != StatusCode::UNAUTHORIZED,
        "the endpoint must be reachable without a token, got {status}"
    );

    app.cleanup().await;
}
