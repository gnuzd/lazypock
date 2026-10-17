//! API keys: creation, revocation, and the scoped access a key grants.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::{json, Value};

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

async fn sql(app: &TestApp, statement: &str) {
    let client = app.client().await;
    client
        .batch_execute(statement)
        .await
        .expect("the test statement should apply");
}

/// Create a key as the superuser, returning `(raw, id)`.
async fn create_key(app: &TestApp, token: &str, body: Value) -> (String, String) {
    let reply = call_authed(
        &router(app).await,
        Method::POST,
        "/api/settings/api-keys",
        Some(body),
        token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a created key");
    let raw = body["api_key"].as_str().expect("the raw key").to_string();
    let id = body["items"][0]["id"].as_str().expect("an id").to_string();
    (raw, id)
}

#[tokio::test]
async fn every_api_key_call_needs_a_superuser() {
    let Some(app) = test_app("api_keys_auth").await else {
        return;
    };

    for (method, uri, body) in [
        (Method::GET, "/api/settings/api-keys", None),
        (Method::POST, "/api/settings/api-keys", Some(json!({}))),
        (Method::DELETE, "/api/settings/api-keys/x", None),
    ] {
        let reply = call(&router(&app).await, method.clone(), uri, body).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {uri}");
    }

    app.cleanup().await;
}

#[tokio::test]
async fn a_key_is_shown_once_and_only_its_digest_is_stored() {
    let Some(app) = test_app("api_keys_create").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let (raw, id) = create_key(&app, &token, json!({ "expiresInDays": 30 })).await;
    assert!(raw.starts_with("lazypock_"), "{raw}");
    assert!(raw.len() > 20, "{raw}");

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/settings/api-keys",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a list");
    let item = &body["items"][0];
    assert_eq!(item["id"], json!(id));
    assert_eq!(item["revoked"], json!(false));
    assert!(item["expires_at"].is_string(), "{item}");
    // The digest is never returned, and neither is the raw key.
    assert!(item.get("hash").is_none(), "{item}");
    assert!(
        !body.to_string().contains(&raw),
        "the raw key must not come back"
    );

    // The settings read must not leak the stored key material either.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/settings",
        None,
        &token,
    )
    .await;
    let settings = reply.body.expect("settings");
    assert!(
        settings.get("auth.api_keys").is_none(),
        "the key store must not be an application setting: {settings}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_key_reaches_collection_listing_but_not_superuser_endpoints() {
    let Some(app) = test_app("api_keys_scope").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let (raw, _id) = create_key(&app, &token, json!({})).await;

    // As a bearer, the key lists collections — the codegen the key exists for.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections",
        None,
        &raw,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert!(reply.body.expect("a list")["items"].is_array());

    // And in the query parameter, which PocketBase's SDK can use.
    let reply = support::get(
        &router(&app).await,
        &format!("/api/collections?api_key={raw}"),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    // It is not a superuser: settings and crons stay out of reach.
    for uri in ["/api/settings", "/api/crons", "/api/logs"] {
        let reply = call_authed(&router(&app).await, Method::GET, uri, None, &raw).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "an API key must not reach {uri}"
        );
    }

    // A wrong key is a bad credential, not an anonymous caller with no credential.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections",
        None,
        "lazypock_0000",
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn revoking_a_key_stops_it_working() {
    let Some(app) = test_app("api_keys_revoke").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let (raw, id) = create_key(&app, &token, json!({})).await;

    // It works before revocation.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections",
        None,
        &raw,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);

    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        &format!("/api/settings/api-keys/{id}"),
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    // It does not work after.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections",
        None,
        &raw,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::UNAUTHORIZED,
        "a revoked key must not authenticate, and must say so"
    );

    // And it is still listed, marked revoked.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/settings/api-keys",
        None,
        &token,
    )
    .await;
    let body = reply.body.expect("a list");
    assert_eq!(body["items"][0]["revoked"], json!(true));

    // Revoking an unknown id is a 404, not a silent success.
    let reply = call_authed(
        &router(&app).await,
        Method::DELETE,
        "/api/settings/api-keys/nope",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    app.cleanup().await;
}

#[tokio::test]
async fn an_expired_key_does_not_authenticate() {
    let Some(app) = test_app("api_keys_expiry").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let (raw, _id) = create_key(&app, &token, json!({ "expiresInDays": 30 })).await;

    // Move the stored expiry into the past, which is what waiting a month does.
    sql(
        &app,
        "update _api_keys set expires_at = now() - interval '1 day'",
    )
    .await;

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections",
        None,
        &raw,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::UNAUTHORIZED,
        "an expired key must not authenticate"
    );

    app.cleanup().await;
}

/// A value carrying the key prefix is a key attempt, so a wrong or revoked one is a 401
/// rather than a quiet demotion to anonymous — which kept a revoked key working against
/// everything public and made a typo look like a permissions problem.
#[tokio::test]
async fn a_key_that_is_not_stored_is_refused_rather_than_ignored() {
    let Some(app) = test_app("api_keys_unknown").await else {
        return;
    };

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/collections",
        None,
        "lazypock_00000000000000000000000000000000",
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{:?}", reply.body);
    assert!(
        reply
            .body
            .as_ref()
            .and_then(|body| body["message"].as_str())
            .is_some_and(|message| message.contains("API key")),
        "the refusal must name the credential: {:?}",
        reply.body
    );

    app.cleanup().await;
}

/// Two keys created at the same moment both survive. The store used to be one JSON array
/// written read-modify-write, so a concurrent create silently dropped one of them.
#[tokio::test]
async fn two_keys_created_at_once_both_survive() {
    let Some(app) = test_app("api_keys_concurrent").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let (first, second) = tokio::join!(
        create_key(&app, &token, json!({})),
        create_key(&app, &token, json!({})),
    );
    assert_ne!(first.0, second.0, "two distinct keys");

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/settings/api-keys",
        None,
        &token,
    )
    .await;
    let body = reply.body.expect("a list");
    assert_eq!(
        body["items"].as_array().expect("items").len(),
        2,
        "neither create may be lost: {body}"
    );

    // Both work, which is the property that matters.
    for raw in [&first.0, &second.0] {
        let reply = call_authed(
            &router(&app).await,
            Method::GET,
            "/api/collections",
            None,
            raw,
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK, "{raw} should authenticate");
    }

    app.cleanup().await;
}
