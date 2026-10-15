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

/// Run a statement against the test's schema.
async fn sql(app: &TestApp, statement: &str) {
    let client = app.client().await;
    client
        .batch_execute(statement)
        .await
        .expect("the statement should apply");
}

/// A real, decodable PNG: upload validation reads the header, thumbnails decode it.
fn png(width: u32, height: u32) -> Vec<u8> {
    let raw = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 120, 200, 255]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(raw)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode png");
    bytes
}

/// Upload one PNG, the way the Studio's record form does.
async fn upload_png(app: &TestApp, token: &str) -> support::Reply {
    const BOUNDARY: &str = "----lazypockSettingsBoundary";
    let bytes = png(8, 8);

    let mut body = Vec::new();
    body.extend_from_slice(
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\";                  filename=\"cover.png\"\r\nContent-Type: image/png\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

    let request = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/api/files")
        .header(
            axum::http::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(axum::body::Body::from(body))
        .expect("request");

    let response = tower::ServiceExt::oneshot(router(app).await, request)
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body")
        .to_vec();
    support::Reply {
        status,
        headers: Default::default(),
        body: serde_json::from_slice(&bytes).ok(),
    }
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
    // Whatever the environment owns is named (and locked by the page); it can only ever be
    // a subset of the fields the page knows, because a field nothing configured is
    // editable — which is what makes "save from the Studio" possible at all.
    let locked = body["configured_from_env"].as_array().expect("a list");
    for field in locked {
        assert!(
            [
                "backend",
                "endpoint",
                "bucket",
                "region",
                "access_key_id",
                "secret_access_key",
                "public_base_url"
            ]
            .contains(&field.as_str().expect("a field name")),
            "unexpected environment-managed field {field}: {body}"
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
async fn saving_storage_settings_takes_effect_without_a_restart() {
    let Some(app) = test_app("settings_storage_write").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // What the Studio sends: its whole form, with a blank secret meaning "keep the stored
    // one".
    let reply = call_authed(
        &router(&app).await,
        Method::PATCH,
        "/api/settings/storage",
        Some(json!({
            "backend": "local",
            "region": "auto",
            "public_base_url": "https://cdn.example.com",
            "prefix": "",
            "force_path_style": true,
            "presign_ttl": 900,
            "secret_access_key": ""
        })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a view");
    assert_eq!(
        body["public_base_url"],
        json!("https://cdn.example.com"),
        "{body}"
    );

    // It was stored as a setting, so a restart keeps it.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/settings",
        None,
        &token,
    )
    .await;
    let settings = reply.body.expect("settings");
    assert_eq!(
        settings["storage"]["public_base_url"],
        json!("https://cdn.example.com"),
        "{settings}"
    );

    // And it is **live**: the next upload hands out a URL on the CDN base, with no restart
    // in between. That is the whole point of saving it here.
    let mut client = app.client().await;
    lazypock_db::collections::create(
        &mut client,
        lazypock_db::collections::CreateCollection {
            name: "posts".into(),
            collection_type: lazypock_db::types::CollectionType::Base,
            fields: vec![lazypock_db::collections::CreateField {
                name: "title".into(),
                field_type: lazypock_db::types::FieldType::Text,
                required: false,
                unique: false,
                options: json!({}),
            }],
            rules: json!({}),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("create posts");
    sql(
        &app,
        "insert into posts (id, title) values ('p1', 'a post')",
    )
    .await;

    let reply = upload_png(&app, &token).await;
    let url = reply.body.expect("a file")["url"]
        .as_str()
        .expect("a url")
        .to_string();
    assert!(
        url.starts_with("https://cdn.example.com/api/files/"),
        "the saved base URL applies to the next upload: {url}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn storage_settings_that_cannot_be_honoured_are_refused() {
    let Some(app) = test_app("settings_storage_unsupported").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // Storing these would look like they worked. This server addresses an object as
    // `<endpoint>/<bucket>/<key>` and has no key prefix, so both are refused with the
    // reason.
    for body in [
        json!({ "force_path_style": false }),
        json!({ "prefix": "lazypock/my-app/" }),
    ] {
        let reply = call_authed(
            &router(&app).await,
            Method::PATCH,
            "/api/settings/storage",
            Some(body.clone()),
            &token,
        )
        .await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{body}: {:?}",
            reply.body
        );

        let reply = call_authed(
            &router(&app).await,
            Method::GET,
            "/api/settings",
            None,
            &token,
        )
        .await;
        assert!(
            reply.body.expect("settings").get("storage").is_none(),
            "nothing should have been stored for {body}"
        );
    }

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
