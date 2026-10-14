//! HTTP-level tests for the authentication endpoints.
//!
//! Auth *collections* cannot have records created yet (the password/email
//! columns are not writable), so these cover the superuser flow, which is
//! complete end to end: `superuser create` → login → bearer → refresh.

mod support;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use lazypock_auth::superuser;
use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

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

// ------------------------------------------------- auth collection accounts

/// Create a `users` auth collection.
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
            // A realistic auth collection: open registration, but everything else
            // limited to the account itself. With no rules at all, default-deny
            // would refuse even signup — which is what the plan specifies.
            rules: json!({
                "createRule": "",
                "listRule": "id = @request.auth.id",
                "viewRule": "id = @request.auth.id",
                "updateRule": "id = @request.auth.id",
                "deleteRule": "id = @request.auth.id",
            }),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("create the users collection");
    app.reload_schema().await;
}

async fn register(app: &TestApp, body: Value) -> support::Reply {
    post(&router(app).await, "/api/collections/users/records", body).await
}

#[tokio::test]
async fn an_account_can_be_registered_and_used_to_log_in() {
    let Some(app) = test_app("http_auth_register").await else {
        return;
    };
    seed_auth_collection(&app).await;

    let reply = register(
        &app,
        json!({
            "email": "Alice@Example.com",
            "password": "correct-horse",
            "passwordConfirm": "correct-horse",
            "name": "Alice",
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let record = reply.body.expect("body");
    assert_eq!(record["name"], json!("Alice"));
    assert_eq!(record["email"], json!("Alice@Example.com"));
    assert_eq!(record["verified"], json!(false));

    // The password never comes back, in any form.
    let serialized = record.to_string();
    for secret in ["correct-horse", "argon2", "password_hash", "token_key"] {
        assert!(
            !serialized.contains(secret),
            "the created record leaked {secret}: {serialized}"
        );
    }

    // Log in with it, case-insensitively on the email.
    let reply = post(
        &router(&app).await,
        "/api/collections/users/auth-with-password",
        json!({ "identity": "alice@example.com", "password": "correct-horse" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("body");
    assert_eq!(body["record"]["id"], record["id"]);
    assert_eq!(body["record"]["email"], json!("Alice@Example.com"));

    let token = body["token"].as_str().expect("token").to_string();
    assert_eq!(
        with_bearer(&app, "/api/health", &token).await,
        StatusCode::OK
    );

    app.cleanup().await;
}

#[tokio::test]
async fn registration_is_validated() {
    let Some(app) = test_app("http_auth_register_validation").await else {
        return;
    };
    seed_auth_collection(&app).await;

    let cases: &[(Value, &str, &str)] = &[
        (
            json!({"email": "a@example.com"}),
            "password",
            "validation_required",
        ),
        (
            json!({"email": "a@example.com", "password": "short"}),
            "password",
            "validation_min_text",
        ),
        (
            json!({"email": "nope", "password": "correct-horse"}),
            "email",
            "validation_invalid_email",
        ),
        (
            json!({"password": "correct-horse"}),
            "email",
            "validation_required",
        ),
        (
            json!({"email": "a@example.com", "password": "correct-horse", "passwordConfirm": "different"}),
            "passwordConfirm",
            "validation_mismatch",
        ),
        (
            json!({"email": "a@example.com", "password": "correct-horse", "nope": 1}),
            "nope",
            "validation_unknown_field",
        ),
    ];

    for (body, field, code) in cases {
        let reply = register(&app, body.clone()).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "for {body}");
        let response = reply.body.expect("body");
        assert_eq!(
            response["data"][field]["code"],
            json!(code),
            "for {body}: {response}"
        );
    }

    app.cleanup().await;
}

#[tokio::test]
async fn emails_are_unique_across_accounts() {
    let Some(app) = test_app("http_auth_register_unique").await else {
        return;
    };
    seed_auth_collection(&app).await;

    let first = register(
        &app,
        json!({"email": "a@example.com", "password": "correct-horse"}),
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED);

    // Case-insensitively too: login looks the account up by lower(email), so two
    // accounts sharing an address would make that lookup ambiguous.
    let duplicate = register(
        &app,
        json!({"email": "A@Example.com", "password": "another-one"}),
    )
    .await;
    assert_eq!(
        duplicate.status,
        StatusCode::CONFLICT,
        "{:?}",
        duplicate.body
    );

    app.cleanup().await;
}

#[tokio::test]
async fn changing_a_password_ends_existing_sessions() {
    let Some(app) = test_app("http_auth_register_password").await else {
        return;
    };
    seed_auth_collection(&app).await;
    let router = router(&app).await;

    let record = register(
        &app,
        json!({"email": "a@example.com", "password": "correct-horse"}),
    )
    .await
    .body
    .expect("body");
    let id = record["id"].as_str().expect("id").to_string();

    let session = post(
        &router,
        "/api/collections/users/auth-with-password",
        json!({ "identity": "a@example.com", "password": "correct-horse" }),
    )
    .await
    .body
    .expect("body");
    // Changing a password is an authenticated action: `updateRule` is
    // `id = @request.auth.id`, so an anonymous request is refused by the
    // database before the handler ever sees it.
    let access = session["token"].as_str().expect("token").to_string();
    let refresh = session["refreshToken"]
        .as_str()
        .expect("refresh")
        .to_string();

    // Change the password through the ordinary records API, as the account.
    let request = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/api/collections/users/records/{id}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {access}"))
        .body(Body::from(
            json!({ "password": "a-new-password" }).to_string(),
        ))
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // The old password stops working and the new one starts.
    let old = post(
        &router,
        "/api/collections/users/auth-with-password",
        json!({ "identity": "a@example.com", "password": "correct-horse" }),
    )
    .await;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);
    let new = post(
        &router,
        "/api/collections/users/auth-with-password",
        json!({ "identity": "a@example.com", "password": "a-new-password" }),
    )
    .await;
    assert_eq!(new.status, StatusCode::OK, "{:?}", new.body);

    // The session opened before the change is gone, not merely stale.
    let replay = post(
        &router,
        "/api/collections/users/auth-refresh",
        json!({ "refreshToken": refresh }),
    )
    .await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED, "{:?}", replay.body);

    app.cleanup().await;
}

#[tokio::test]
async fn credential_columns_are_neither_returned_nor_filterable() {
    let Some(app) = test_app("http_auth_credentials_hidden").await else {
        return;
    };
    seed_auth_collection(&app).await;
    let router = router(&app).await;

    register(
        &app,
        json!({"email": "a@example.com", "password": "correct-horse"}),
    )
    .await;

    // The default projection must not carry credentials. This was a real leak:
    // the columns were in the filter context, so `fields=*` returned them.
    let reply = get(&router, "/api/collections/users/records").await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let serialized = reply.body.expect("body").to_string();
    for secret in ["password_hash", "token_key", "argon2", "correct-horse"] {
        assert!(
            !serialized.contains(secret),
            "the listing leaked {secret}: {serialized}"
        );
    }

    // ... and neither may be requested or filtered on: they are unknown fields.
    for uri in [
        "/api/collections/users/records?fields=password_hash",
        "/api/collections/users/records?fields=id,password_hash",
        "/api/collections/users/records?filter=password_hash%3D%22x%22",
        "/api/collections/users/records?filter=token_key%3D%22x%22",
    ] {
        let reply = get(&router, uri).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{uri} should have been refused"
        );
    }

    app.cleanup().await;
}

// ------------------------------------------------ access control, end to end

/// Create a `notes` collection whose rows each account owns.
async fn seed_notes(app: &TestApp) {
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: "notes".into(),
            collection_type: CollectionType::Base,
            fields: vec![
                CreateField {
                    name: "author".into(),
                    field_type: FieldType::Text,
                    required: false,
                    unique: false,
                    options: json!({}),
                },
                CreateField {
                    name: "body".into(),
                    field_type: FieldType::Text,
                    required: false,
                    unique: false,
                    options: json!({}),
                },
            ],
            rules: json!({
                "listRule": "author = @request.auth.id",
                "viewRule": "author = @request.auth.id",
                "createRule": "author = @request.auth.id",
                "updateRule": "author = @request.auth.id",
                "deleteRule": "author = @request.auth.id",
            }),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("create notes");
    app.reload_schema().await;
}

/// Register an account and return its access token.
async fn account(app: &TestApp, email: &str) -> String {
    register(app, json!({"email": email, "password": "correct-horse"})).await;
    post(
        &router(app).await,
        "/api/collections/users/auth-with-password",
        json!({ "identity": email, "password": "correct-horse" }),
    )
    .await
    .body
    .expect("body")["token"]
        .as_str()
        .expect("token")
        .to_string()
}

async fn list_notes(app: &TestApp, token: &str) -> Value {
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/collections/notes/records")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let response = router(app).await.oneshot(request).await.expect("response");
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

async fn create_note(app: &TestApp, token: &str, author: &str, body: &str) -> StatusCode {
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/collections/notes/records")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            json!({ "author": author, "body": body }).to_string(),
        ))
        .expect("request");
    router(app)
        .await
        .oneshot(request)
        .await
        .expect("response")
        .status()
}

#[tokio::test]
async fn each_account_only_sees_its_own_records() {
    let Some(app) = test_app("http_rls_end_to_end").await else {
        return;
    };
    seed_auth_collection(&app).await;
    seed_notes(&app).await;

    let alice = account(&app, "alice@example.com").await;
    let bob = account(&app, "bob@example.com").await;
    // Each account's id, taken from the token it was issued.
    let alice_id = token_subject(&alice);
    let bob_id = token_subject(&bob);

    assert_eq!(
        create_note(&app, &alice, &alice_id, "alice's note").await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_note(&app, &bob, &bob_id, "bob's note").await,
        StatusCode::CREATED
    );

    // Each sees exactly one note, their own. Postgres does the filtering.
    let alice_notes = list_notes(&app, &alice).await;
    assert_eq!(alice_notes["totalItems"], json!(1));
    assert_eq!(alice_notes["items"][0]["body"], json!("alice's note"));

    let bob_notes = list_notes(&app, &bob).await;
    assert_eq!(bob_notes["totalItems"], json!(1));
    assert_eq!(bob_notes["items"][0]["body"], json!("bob's note"));

    // Writing a note owned by somebody else is refused by the WITH CHECK.
    assert_eq!(
        create_note(&app, &alice, &bob_id, "forged").await,
        StatusCode::FORBIDDEN,
        "the create rule must refuse a row owned by another account"
    );

    app.cleanup().await;
}

/// The `sub` claim of an access token, decoded without verifying.
fn token_subject(token: &str) -> String {
    use base64::Engine;
    let payload = token.split('.').nth(1).expect("payload");
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .expect("decode");
    let claims: Value = serde_json::from_slice(&bytes).expect("claims");
    claims["sub"].as_str().expect("sub").to_string()
}
