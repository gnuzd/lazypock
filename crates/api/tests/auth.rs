//! HTTP-level tests for the authentication endpoints.
//!
//! Auth *collections* cannot have records created yet (the password/email
//! columns are not writable), so these cover the superuser flow, which is
//! complete end to end: `superuser create` → login → bearer → refresh.

mod support;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

use lazypock_auth::superuser;

use support::{get, post, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

async fn login(app: &TestApp, identity: &str, password: &str) -> support::Reply {
    let router = router(app).await;
    post(
        &router,
        "/api/collections/_superusers/auth-with-password",
        json!({ "identity": identity, "password": password }),
    )
    .await
}

async fn with_bearer(app: &TestApp, uri: &str, token: &str) -> StatusCode {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    router(app)
        .await
        .oneshot(request)
        .await
        .expect("response")
        .status()
}

#[tokio::test]
async fn a_superuser_can_log_in_over_http() {
    let Some(app) = test_app("http_auth_superuser").await else {
        return;
    };
    let client = app.client().await;
    superuser::create(&client, "admin@example.com", "correct-horse")
        .await
        .expect("create");

    let reply = login(&app, "ADMIN@example.com", "correct-horse").await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("body");

    assert!(body["token"].is_string());
    assert!(body["refreshToken"].is_string());
    assert_eq!(body["expiresIn"], json!(900));
    assert_eq!(body["record"]["email"], json!("admin@example.com"));
    assert_eq!(body["record"]["collectionName"], json!("_superusers"));
    assert!(
        !body.to_string().contains("argon2"),
        "the password hash must never be returned: {body}"
    );

    // A wrong password is a 401 in the standard envelope, and says no more than
    // an unknown account does.
    let wrong = login(&app, "admin@example.com", "wrong").await;
    let unknown = login(&app, "ghost@example.com", "correct-horse").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);

    let wrong_body = wrong.body.expect("body");
    let unknown_body = unknown.body.expect("body");
    assert_eq!(wrong_body["status"], json!(401));
    assert_eq!(
        wrong_body["message"], unknown_body["message"],
        "the two failures must not be distinguishable"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_valid_token_is_accepted_and_a_bad_one_is_refused() {
    let Some(app) = test_app("http_auth_bearer").await else {
        return;
    };
    let client = app.client().await;
    superuser::create(&client, "admin@example.com", "correct-horse")
        .await
        .expect("create");

    let token = login(&app, "admin@example.com", "correct-horse")
        .await
        .body
        .expect("body")["token"]
        .as_str()
        .expect("token")
        .to_string();

    // No token at all is simply anonymous.
    assert_eq!(with_bearer(&app, "/api/health", "").await, StatusCode::OK);
    assert_eq!(
        get(&router(&app).await, "/api/health").await.status,
        StatusCode::OK
    );

    // A valid token is accepted.
    assert_eq!(
        with_bearer(&app, "/api/health", &token).await,
        StatusCode::OK
    );

    // A token that is present but unusable is a 401, not a silent downgrade.
    for bad in ["not-a-token", "a.b.c", "eyJhbGciOiJFZERTQSJ9.e30.x"] {
        assert_eq!(
            with_bearer(&app, "/api/health", bad).await,
            StatusCode::UNAUTHORIZED,
            "{bad:?} should have been refused"
        );
    }

    app.cleanup().await;
}

#[tokio::test]
async fn refresh_over_http_rotates_and_detects_replay() {
    let Some(app) = test_app("http_auth_refresh").await else {
        return;
    };
    let client = app.client().await;
    superuser::create(&client, "admin@example.com", "correct-horse")
        .await
        .expect("create");
    let router = router(&app).await;

    let first = login(&app, "admin@example.com", "correct-horse")
        .await
        .body
        .expect("body");
    let first_refresh = first["refreshToken"].as_str().expect("refresh").to_string();

    let reply = post(
        &router,
        "/api/collections/_superusers/auth-refresh",
        json!({ "refreshToken": first_refresh }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let second = reply.body.expect("body");
    let second_refresh = second["refreshToken"]
        .as_str()
        .expect("refresh")
        .to_string();
    assert_ne!(second_refresh, first_refresh, "the token must rotate");
    // A fresh access token is issued and works.
    let fresh_access = second["token"].as_str().expect("token").to_string();
    assert_eq!(
        with_bearer(&app, "/api/health", &fresh_access).await,
        StatusCode::OK
    );

    // Replaying the spent token is refused.
    let reply = post(
        &router,
        "/api/collections/_superusers/auth-refresh",
        json!({ "refreshToken": first_refresh }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn refresh_accepts_a_bearer_token_like_the_pocketbase_sdk() {
    let Some(app) = test_app("http_auth_refresh_bearer").await else {
        return;
    };
    let client = app.client().await;
    superuser::create(&client, "admin@example.com", "correct-horse")
        .await
        .expect("create");

    let refresh = login(&app, "admin@example.com", "correct-horse")
        .await
        .body
        .expect("body")["refreshToken"]
        .as_str()
        .expect("refresh")
        .to_string();

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/collections/_superusers/auth-refresh")
        .header(header::AUTHORIZATION, format!("Bearer {refresh}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("request");
    let response = router(&app).await.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // ... and nothing at all is a 400 with a message.
    let reply = post(
        &router(&app).await,
        "/api/collections/_superusers/auth-refresh",
        json!({}),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);

    app.cleanup().await;
}

#[tokio::test]
async fn the_public_key_is_published_without_the_private_half() {
    let Some(app) = test_app("http_auth_jwks").await else {
        return;
    };
    let router = router(&app).await;

    for uri in ["/api/jwks", "/.well-known/jwks.json"] {
        let reply = get(&router, uri).await;
        assert_eq!(reply.status, StatusCode::OK, "{uri}");
        let body = reply.body.expect("body");
        assert_eq!(body["keys"][0]["kty"], json!("OKP"), "{uri}");
        assert_eq!(body["keys"][0]["crv"], json!("Ed25519"), "{uri}");
        assert_eq!(body["keys"][0]["alg"], json!("EdDSA"), "{uri}");
        assert!(body["keys"][0]["kid"].is_string(), "{uri}");
        assert!(body["keys"][0]["x"].is_string(), "{uri}");
        // Only public material: no private key fields.
        let serialized = body.to_string();
        for secret in ["\"d\"", "\"p\"", "\"q\"", "der", "private"] {
            assert!(
                !serialized.contains(secret),
                "{uri} published {secret}: {serialized}"
            );
        }
    }

    app.cleanup().await;
}

#[tokio::test]
async fn auth_methods_report_what_is_enabled() {
    let Some(app) = test_app("http_auth_methods").await else {
        return;
    };
    let router = router(&app).await;

    let reply = get(&router, "/api/collections/_superusers/auth-methods").await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.body.expect("body");
    assert_eq!(body["password"]["enabled"], json!(true));
    // Disabled methods are reported as off rather than omitted.
    assert_eq!(body["oauth2"]["enabled"], json!(false));
    assert_eq!(body["mfa"]["enabled"], json!(false));

    // An unknown collection is a 404, not an empty answer.
    let reply = get(&router, "/api/collections/nope/auth-methods").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    app.cleanup().await;
}

#[tokio::test]
async fn a_malformed_login_body_is_rejected_clearly() {
    let Some(app) = test_app("http_auth_bad_body").await else {
        return;
    };
    let router = router(&app).await;

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/collections/_superusers/auth-with-password")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{ not json"))
        .expect("request");
    let response = router.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    app.cleanup().await;
}

#[tokio::test]
async fn an_unknown_value_is_reported_once_without_leaking_internals() {
    // A login for a collection that does not exist should not reveal whether it
    // exists, and must not leak SQL.
    let Some(app) = test_app("http_auth_unknown_collection").await else {
        return;
    };
    let reply = login(&app, "a@example.com", "whatever").await;
    let body = reply.body.expect("body");
    assert!(
        !body.to_string().contains("select "),
        "the response leaked SQL: {body}"
    );

    app.cleanup().await;
}
