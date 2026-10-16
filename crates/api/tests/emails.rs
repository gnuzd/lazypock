//! The email flows: verification, password reset and email change.
//!
//! The request half mints a token into `_otps` and queues a `mail` job. The tests
//! read the raw token out of that job's payload rather than sending mail: the token
//! is only ever stored hashed, which is the property under test elsewhere.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::json;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{call_authed, post, test_app, TestApp};

async fn app_router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
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

async fn register(app: &TestApp, email: &str, password: &str) {
    let reply = post(
        &app_router(app).await,
        "/api/collections/users/records",
        json!({ "email": email, "password": password, "passwordConfirm": password }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
}

async fn login(app: &TestApp, email: &str, password: &str) -> support::Reply {
    post(
        &app_router(app).await,
        "/api/collections/users/auth-with-password",
        json!({ "identity": email, "password": password }),
    )
    .await
}

/// The raw token of the most recently queued mail job.
async fn latest_mail_token(app: &TestApp) -> String {
    let row = app
        .client()
        .await
        .query_one(
            "select payload #>> '{assigns,token}' as token \
               from _jobs where kind = 'mail' order by created desc limit 1",
            &[],
        )
        .await
        .expect("a queued mail job");
    row.get::<_, Option<String>>("token").expect("a token")
}

async fn mail_job_count(app: &TestApp) -> i64 {
    app.client()
        .await
        .query_one("select count(*) as n from _jobs where kind = 'mail'", &[])
        .await
        .expect("count")
        .get("n")
}

#[tokio::test]
async fn requesting_and_confirming_verification_marks_the_account_verified() {
    let Some(app) = test_app("emails_verify").await else {
        return;
    };
    seed_auth_collection(&app).await;
    register(&app, "ada@example.com", "correct-horse").await;

    let reply = post(
        &app_router(&app).await,
        "/api/collections/users/request-verification",
        json!({ "email": "ada@example.com" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);
    assert_eq!(
        mail_job_count(&app).await,
        1,
        "a verification mail is queued"
    );

    let token = latest_mail_token(&app).await;
    let reply = post(
        &app_router(&app).await,
        "/api/collections/users/confirm-verification",
        json!({ "token": token }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let record = reply.body.expect("record");
    assert_eq!(record["record"]["verified"], json!(true), "{record}");
    // The credential columns never come back.
    assert!(!record.to_string().contains("password_hash"), "{record}");

    // Single-use: the row was deleted by the statement that read it.
    let reply = post(
        &app_router(&app).await,
        "/api/collections/users/confirm-verification",
        json!({ "token": token }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn a_request_for_an_unknown_address_answers_204_and_sends_nothing() {
    let Some(app) = test_app("emails_unknown").await else {
        return;
    };
    seed_auth_collection(&app).await;

    // 204 either way: this must not be usable to find out which addresses exist.
    let reply = post(
        &app_router(&app).await,
        "/api/collections/users/request-verification",
        json!({ "email": "nobody@example.com" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);
    assert_eq!(mail_job_count(&app).await, 0, "nothing was queued");

    app.cleanup().await;
}

#[tokio::test]
async fn a_password_reset_changes_the_password_and_revokes_the_refresh_token() {
    let Some(app) = test_app("emails_reset").await else {
        return;
    };
    seed_auth_collection(&app).await;
    register(&app, "ada@example.com", "correct-horse").await;

    let session = login(&app, "ada@example.com", "correct-horse").await;
    assert_eq!(session.status, StatusCode::OK, "{:?}", session.body);
    let refresh = session.body.expect("auth")["refreshToken"]
        .as_str()
        .expect("refresh token")
        .to_string();

    let reply = post(
        &app_router(&app).await,
        "/api/collections/users/request-password-reset",
        json!({ "email": "ada@example.com" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    let token = latest_mail_token(&app).await;
    let reply = post(
        &app_router(&app).await,
        "/api/collections/users/confirm-password-reset",
        json!({
            "token": token,
            "password": "new-correct-horse",
            "passwordConfirm": "new-correct-horse",
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    // The new password works; the old one does not.
    assert_eq!(
        login(&app, "ada@example.com", "new-correct-horse")
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        login(&app, "ada@example.com", "correct-horse").await.status,
        StatusCode::UNAUTHORIZED
    );

    // The reset ended the sessions the old password created.
    let reply = post(
        &app_router(&app).await,
        "/api/collections/users/auth-refresh",
        json!({ "refreshToken": refresh }),
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::UNAUTHORIZED,
        "the old refresh token must be revoked: {:?}",
        reply.body
    );

    app.cleanup().await;
}

#[tokio::test]
async fn confirm_password_reset_validates_the_new_password() {
    let Some(app) = test_app("emails_reset_validation").await else {
        return;
    };
    seed_auth_collection(&app).await;
    register(&app, "ada@example.com", "correct-horse").await;

    post(
        &app_router(&app).await,
        "/api/collections/users/request-password-reset",
        json!({ "email": "ada@example.com" }),
    )
    .await;
    let token = latest_mail_token(&app).await;

    for bad in [
        json!({ "token": token, "password": "short", "passwordConfirm": "short" }),
        json!({ "token": token, "password": "long-enough", "passwordConfirm": "different" }),
    ] {
        let reply = post(
            &app_router(&app).await,
            "/api/collections/users/confirm-password-reset",
            bad,
        )
        .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    }

    // A rejected password did not burn the token.
    let reply = post(
        &app_router(&app).await,
        "/api/collections/users/confirm-password-reset",
        json!({
            "token": token,
            "password": "long-enough-password",
            "passwordConfirm": "long-enough-password",
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn changing_an_email_needs_auth_and_the_current_password() {
    let Some(app) = test_app("emails_change").await else {
        return;
    };
    seed_auth_collection(&app).await;
    register(&app, "ada@example.com", "correct-horse").await;

    let session = login(&app, "ada@example.com", "correct-horse").await;
    assert_eq!(session.status, StatusCode::OK, "{:?}", session.body);
    let access = session.body.expect("auth")["token"]
        .as_str()
        .expect("token")
        .to_string();

    // Public callers cannot start a change.
    let reply = post(
        &app_router(&app).await,
        "/api/collections/users/request-email-change",
        json!({ "newEmail": "new@example.com" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{:?}", reply.body);

    let reply = call_authed(
        &app_router(&app).await,
        Method::POST,
        "/api/collections/users/request-email-change",
        Some(json!({ "newEmail": "new@example.com" })),
        &access,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);

    let token = latest_mail_token(&app).await;

    // A mistyped password must not burn the link.
    let reply = call_authed(
        &app_router(&app).await,
        Method::POST,
        "/api/collections/users/confirm-email-change",
        Some(json!({ "token": token, "password": "wrong" })),
        &access,
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{:?}", reply.body);

    let reply = call_authed(
        &app_router(&app).await,
        Method::POST,
        "/api/collections/users/confirm-email-change",
        Some(json!({ "token": token, "password": "correct-horse" })),
        &access,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(
        reply.body.expect("record")["record"]["email"],
        json!("new@example.com")
    );

    // The new address is the one that works now.
    assert_eq!(
        login(&app, "new@example.com", "correct-horse").await.status,
        StatusCode::OK
    );

    app.cleanup().await;
}
