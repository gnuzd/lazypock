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

#[tokio::test]
async fn the_files_storage_page_reads_the_running_configuration() {
    let Some(app) = test_app("settings_storage_read").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // The Studio's Files Storage page calls this on mount; a 404 there reads as a broken
    // server rather than as a configuration that lives in the environment.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/settings/storage",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a view");

    assert_eq!(body["backend"], json!("local"), "{body}");
    // The secret is never echoed, only whether there is one.
    assert_eq!(body["secret_access_key"], json!(""), "{body}");
    assert_eq!(body["secret_set"], json!(false), "{body}");
    // Every field is environment-managed, which is what locks the form.
    let locked = body["configured_from_env"].as_array().expect("a list");
    for field in [
        "backend",
        "endpoint",
        "bucket",
        "region",
        "access_key_id",
        "secret_access_key",
        "prefix",
        "public_base_url",
    ] {
        assert!(
            locked.contains(&json!(field)),
            "{field} should be listed as environment-managed: {body}"
        );
    }
    assert_eq!(body["presign_ttl"], json!(900), "{body}");

    // Superuser-only, like the rest of the settings surface.
    let reply = call(
        &router(&app).await,
        Method::GET,
        "/api/settings/storage",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn saving_storage_settings_is_refused_with_the_variables_to_change() {
    let Some(app) = test_app("settings_storage_write").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Accepting this and storing nothing would be worse than refusing it, so the refusal
    // names every variable that has to change instead.
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/settings/storage",
        Some(json!({ "backend": "s3", "bucket": "media" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{:?}", reply.body);

    let message = reply.body.expect("a refusal")["message"]
        .as_str()
        .expect("a message")
        .to_string();
    for variable in [
        "LAZYPOCK_FILES_ADAPTER",
        "LAZYPOCK_S3_ENDPOINT",
        "LAZYPOCK_S3_BUCKET",
        "LAZYPOCK_S3_ACCESS_KEY",
        "LAZYPOCK_S3_SECRET_KEY",
        "LAZYPOCK_FILES_PUBLIC_URL",
    ] {
        assert!(
            message.contains(variable),
            "{variable} missing from {message}"
        );
    }

    // And nothing was written: the setting the page would have saved is absent.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/settings",
        None,
        &token,
    )
    .await;
    let body = reply.body.expect("settings");
    assert!(
        body.get("storage").is_none(),
        "the storage setting must not be stored: {body}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn the_storage_test_round_trips_against_the_configured_backend() {
    let Some(app) = test_app("settings_storage_test").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // A real probe — the local adapter creates and checks its directory — reported as a
    // step, so the page says which part failed rather than "the connection failed".
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/settings/storage/test",
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a report");
    assert_eq!(body["ok"], json!(true), "{body}");
    let steps = body["steps"].as_array().expect("steps");
    assert_eq!(steps[0]["step"], json!("local"), "{body}");
    assert_eq!(steps[0]["ok"], json!(true), "{body}");

    app.cleanup().await;
}
