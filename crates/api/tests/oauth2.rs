//! OAuth2 external auth, end to end against a mock provider.
//!
//! The provider is a loopback HTTP server, so the token exchange and the userinfo
//! read are real requests rather than stubs: what is exercised is the actual
//! code path a deployment runs.

mod support;

use axum::http::{Method, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{call, call_authed, superuser_token, test_app, test_app_with, TestApp};

async fn app_router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// A mock provider: a token endpoint and a userinfo endpoint on one loopback port.
async fn mock_provider() -> String {
    let provider = Router::new()
        .route(
            "/token",
            post(|| async {
                Json(json!({ "access_token": "mock-access-token", "token_type": "Bearer" }))
            }),
        )
        .route(
            "/user",
            get(|| async {
                Json(json!({
                    "sub": "ext-user-1",
                    "email": "mock@example.com",
                    "name": "Mock User",
                    "picture": "https://example.test/avatar.png"
                }))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the mock provider");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, provider).await;
    });

    format!("http://{address}")
}

async fn seed_auth_collection(app: &TestApp) {
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: "users".into(),
            collection_type: CollectionType::Auth,
            fields: vec![CreateField {
                name: "name".into(),
                field_type: FieldType::Text,
                required: false,
                unique: false,
                options: json!({}),
            }],
            rules: json!({
                "createRule": "",
                "listRule": "id = @request.auth.id",
                "viewRule": "id = @request.auth.id",
            }),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("create the users collection");
    app.reload_schema().await;
}

/// Register the mock provider in settings. Returns its base URL.
async fn configure_mock_provider(app: &TestApp, token: &str) -> String {
    let base = mock_provider().await;
    let reply = call_authed(
        &app_router(app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!({
            "oauth2": { "providers": [{
                "name": "mock",
                "config": {
                    "client_id": "mock-client",
                    "client_secret": "mock-secret",
                    "redirect_uri": "http://localhost:4000/api/oauth2-redirect",
                    "authorize_url": format!("{base}/authorize"),
                    "token_url": format!("{base}/token"),
                    "user_url": format!("{base}/user"),
                    "scopes": ["openid", "email"]
                }
            }] }
        })),
        token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    base
}

#[tokio::test]
async fn auth_methods_lists_a_configured_provider_with_pkce() {
    let Some(app) = test_app("oauth2_methods").await else {
        return;
    };
    seed_auth_collection(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let base = configure_mock_provider(&app, &token).await;

    let reply = call(
        &app_router(&app).await,
        Method::GET,
        "/api/collections/users/auth-methods",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("methods");
    assert_eq!(body["oauth2"]["enabled"], json!(true), "{body}");

    let providers = body["oauth2"]["providers"].as_array().expect("providers");
    assert_eq!(providers.len(), 1, "{body}");
    assert_eq!(providers[0]["name"], json!("mock"));

    let auth_url = providers[0]["authURL"].as_str().expect("authURL");
    assert!(
        auth_url.starts_with(&format!("{base}/authorize?")),
        "{auth_url}"
    );
    assert!(
        auth_url.contains("code_challenge_method=S256"),
        "{auth_url}"
    );
    assert!(auth_url.contains("code_challenge="), "{auth_url}");
    assert!(auth_url.contains("state="), "{auth_url}");

    // The verifier is a secret; it goes to the client, never into the URL.
    let verifier = providers[0]["codeVerifier"].as_str().expect("verifier");
    assert!(!verifier.is_empty());
    assert!(!auth_url.contains(verifier), "{auth_url}");

    // Password auth is still reported, and OTP/MFA stay explicitly off.
    assert_eq!(body["password"]["enabled"], json!(true));
    assert_eq!(body["mfa"]["enabled"], json!(false));

    app.cleanup().await;
}

#[tokio::test]
async fn a_sign_in_creates_a_record_links_it_and_reuses_it() {
    let Some(app) = test_app("oauth2_sign_in").await else {
        return;
    };
    seed_auth_collection(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    configure_mock_provider(&app, &token).await;

    let body = json!({
        "provider": "mock",
        "code": "auth-code",
        "codeVerifier": "verifier",
        "redirectUrl": "http://localhost:4000/api/oauth2-redirect",
        "createData": { "name": "From CreateData" }
    });

    let reply = call(
        &app_router(&app).await,
        Method::POST,
        "/api/collections/users/auth-with-oauth2",
        Some(body.clone()),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let first = reply.body.expect("auth");
    assert!(first["token"].is_string(), "{first}");
    assert!(first["refreshToken"].is_string(), "{first}");
    assert_eq!(first["record"]["email"], json!("mock@example.com"));
    // The provider verified the address, so the account starts verified.
    assert_eq!(first["record"]["verified"], json!(true), "{first}");
    // `createData` wins over the provider's name.
    assert_eq!(first["record"]["name"], json!("From CreateData"), "{first}");
    // The random password and the token key never come back.
    let serialized = first.to_string();
    for secret in ["password_hash", "token_key"] {
        assert!(
            !serialized.contains(secret),
            "leaked {secret}: {serialized}"
        );
    }

    let id = first["record"]["id"].as_str().expect("id").to_string();

    // A second sign-in with the same provider identity reuses the account.
    let reply = call(
        &app_router(&app).await,
        Method::POST,
        "/api/collections/users/auth-with-oauth2",
        Some(body),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("auth")["record"]["id"], json!(id));

    app.cleanup().await;
}

#[tokio::test]
async fn create_data_cannot_set_credentials_or_identity() {
    let Some(app) = test_app("oauth2_create_data").await else {
        return;
    };
    seed_auth_collection(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    configure_mock_provider(&app, &token).await;

    let reply = call(
        &app_router(&app).await,
        Method::POST,
        "/api/collections/users/auth-with-oauth2",
        Some(json!({
            "provider": "mock",
            "code": "auth-code",
            "codeVerifier": "verifier",
            "redirectUrl": "http://localhost:4000/api/oauth2-redirect",
            "createData": {
                "email": "attacker@example.com",
                "password": "known-password",
                "verified": false
            }
        })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("auth");

    // The identity is the provider's, and the account cannot be unverified.
    assert_eq!(body["record"]["email"], json!("mock@example.com"), "{body}");
    assert_eq!(body["record"]["verified"], json!(true), "{body}");

    // The planted password does not work; the account signs in through the provider.
    let reply = call(
        &app_router(&app).await,
        Method::POST,
        "/api/collections/users/auth-with-password",
        Some(json!({ "identity": "mock@example.com", "password": "known-password" })),
    )
    .await;
    assert_ne!(reply.status, StatusCode::OK, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn the_redirect_bridge_is_single_use_and_needs_the_flow_origin() {
    // A deployment origin, because the test harness sends no `Origin` or `Host`.
    let Some(app) = test_app_with(
        "oauth2_redirect",
        &[("LAZYPOCK_PUBLIC_URL", "https://app.example")],
    )
    .await
    else {
        return;
    };
    seed_auth_collection(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    configure_mock_provider(&app, &token).await;

    // Starting a flow remembers the origin, keyed by the state it returns.
    let reply = call(
        &app_router(&app).await,
        Method::GET,
        "/api/collections/users/auth-methods",
        None,
    )
    .await;
    let state = reply.body.expect("methods")["oauth2"]["providers"][0]["state"]
        .as_str()
        .expect("state")
        .to_string();

    let reply = call(
        &app_router(&app).await,
        Method::GET,
        &format!("/api/oauth2-redirect?state={state}&code=abc"),
        None,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "the first callback must be answered: {:?}",
        reply.body
    );

    // The flow was consumed, so a replay finds no origin and is refused.
    let reply = call(
        &app_router(&app).await,
        Method::GET,
        &format!("/api/oauth2-redirect?state={state}&code=abc"),
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    app.cleanup().await;
}
