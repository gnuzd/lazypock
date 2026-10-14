//! Application settings.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::json;

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

#[tokio::test]
async fn settings_need_a_superuser() {
    let Some(app) = test_app("settings_auth").await else {
        return;
    };

    for (method, body) in [
        (Method::GET, None),
        (Method::PATCH, Some(json!({ "app_name": "x" }))),
    ] {
        let reply = call(&router(&app).await, method.clone(), "/api/settings", body).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method}");
    }

    app.cleanup().await;
}

#[tokio::test]
async fn settings_round_trip_and_merge_rather_than_replace() {
    let Some(app) = test_app("settings_round_trip").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Nothing configured yet: an empty object, not an error.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/settings",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("settings"), json!({}));

    // Write two groups, then a third in a later call.
    for body in [
        json!({ "app_name": "My App", "cors_origins": ["https://a.example"] }),
        json!({ "mail": { "host": "smtp.example", "port": 587 } }),
    ] {
        let reply = call_authed(
            &router(&app).await,
            Method::PATCH,
            "/api/settings",
            Some(body),
            &token,
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    }

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/settings",
        None,
        &token,
    )
    .await;
    let settings = reply.body.expect("settings");
    assert_eq!(settings["app_name"], json!("My App"));
    assert_eq!(settings["cors_origins"], json!(["https://a.example"]));
    assert_eq!(
        settings["mail"]["host"],
        json!("smtp.example"),
        "a later save must not drop what an earlier one wrote"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_null_setting_is_cleared_rather_than_stored() {
    let Some(app) = test_app("settings_clear").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "app_name": "My App", "cors_origins": ["https://a.example"] })),
        &token,
    )
    .await;

    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "app_name": null })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

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
        settings.get("app_name").is_none(),
        "a cleared setting is absent, so a client can tell unset from null: {settings}"
    );
    assert_eq!(
        settings["cors_origins"],
        json!(["https://a.example"]),
        "clearing one key must not touch another"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_settings_body_that_is_not_an_object_is_refused() {
    let Some(app) = test_app("settings_shape").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!(["not", "an", "object"])),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn the_signing_key_is_neither_exposed_nor_writable() {
    // `_settings` also holds lazypock's own secrets, and the JWT signing key is one
    // of them. The first version of this endpoint returned it — the credential that
    // mints every token — which is the kind of leak a settings API is a natural home
    // for and the reason this test exists at the HTTP level, not only as a unit test
    // of the guard.
    let Some(app) = test_app("settings_signing_key").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // It is in the table.
    let client = app.client().await;
    let stored: i64 = client
        .query_one(
            "select count(*) from _settings where key = 'auth.signing_key'",
            &[],
        )
        .await
        .expect("count")
        .get(0);
    assert_eq!(stored, 1, "the signing key is stored in _settings");

    // It does not come out of a settings read.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/settings",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.body.expect("settings");
    assert!(
        body.get("auth.signing_key").is_none(),
        "the signing key must never leave the server: {body}"
    );
    // Nothing else internal leaks either.
    for (key, _) in body.as_object().expect("an object") {
        assert!(
            !key.starts_with("auth."),
            "{key} is lazypock's own setting and must not be exposed"
        );
    }

    // And it cannot be overwritten through the settings API.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "auth.signing_key": { "der": "attacker", "kid": "x" } })),
        &token,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::BAD_REQUEST,
        "writing an internal setting must be refused: {:?}",
        reply.body
    );

    // The key is unchanged, so authentication still works.
    let reply = call_authed(&router(&app).await, Method::GET, "/api/me", None, &token).await;
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "the token must still verify after the refusal"
    );

    app.cleanup().await;
}
