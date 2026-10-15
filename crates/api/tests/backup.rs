//! Backup and restore over HTTP.

mod support;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

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

/// A `posts` collection with a couple of rows.
async fn seed(app: &TestApp) {
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: "posts".into(),
            collection_type: CollectionType::Base,
            fields: vec![CreateField {
                name: "title".into(),
                field_type: FieldType::Text,
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
    app.reload_schema().await;
    sql(
        app,
        "insert into posts (id, title) values ('p1', 'first'), ('p2', 'second')",
    )
    .await;
}

/// Download an archive.
async fn export(app: &TestApp, token: &str) -> Vec<u8> {
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/export/archive")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let response = router(app).await.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .expect("body")
        .to_vec()
}

/// Upload an archive to the import endpoint.
async fn import(
    app: &TestApp,
    token: &str,
    archive: &[u8],
    extras: &[(&str, &str)],
) -> support::Reply {
    let boundary = "----lazypockBackupBoundary";
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"backup.zip\"\r\nContent-Type: application/zip\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(archive);
    body.extend_from_slice(b"\r\n");
    for (name, value) in extras {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/import")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .expect("request");
    let response = router(app).await.oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .expect("body");
    support::Reply {
        status,
        headers: Default::default(),
        body: if bytes.is_empty() {
            None
        } else {
            serde_json::from_slice(&bytes).ok()
        },
    }
}

#[tokio::test]
async fn backup_and_restore_need_a_superuser() {
    let Some(app) = test_app("backup_auth").await else {
        return;
    };

    for (method, uri) in [
        (Method::GET, "/api/export"),
        (Method::GET, "/api/export/archive"),
        (Method::GET, "/api/import/preflight"),
        (Method::GET, "/api/import/status"),
        (Method::POST, "/api/import"),
        (Method::POST, "/api/import/rollback"),
    ] {
        let reply = call(&router(&app).await, method.clone(), uri, None).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {uri}");
    }

    app.cleanup().await;
}

#[tokio::test]
async fn the_json_export_holds_every_collection_and_its_records() {
    let Some(app) = test_app("backup_export_json").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/export",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a payload");

    let collections = body["collections"].as_array().expect("collections");
    assert_eq!(collections.len(), 1, "only user collections: {body}");
    let posts = &collections[0];
    assert_eq!(posts["name"], json!("posts"));
    assert_eq!(posts["type"], json!("base"));
    // The definition is there...
    assert_eq!(posts["schema"][0]["name"], json!("title"));
    // ...and so are the rows, inline, which is what makes the document restorable.
    let records = posts["records"].as_array().expect("records");
    assert_eq!(records.len(), 2, "{body}");
    let titles: Vec<&str> = records
        .iter()
        .map(|record| record["title"].as_str().expect("a title"))
        .collect();
    assert!(
        titles.contains(&"first") && titles.contains(&"second"),
        "{titles:?}"
    );

    // System tables are never exported: `_superusers` holds password hashes.
    assert!(
        collections
            .iter()
            .all(|collection| !collection["name"].as_str().unwrap_or("").starts_with('_')),
        "{body}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_database_can_be_exported_and_imported_back_over_http() {
    let Some(app) = test_app("backup_http_round_trip").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let archive = export(&app, &token).await;
    assert_eq!(&archive[0..2], b"PK", "the download is a zip");

    // Wipe, then restore through the endpoint.
    sql(&app, "delete from posts").await;

    let reply = import(&app, &token, &archive, &[("atomic", "batch")]).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let report = reply.body.expect("a report");
    assert_eq!(report["imported"][0]["name"], json!("posts"));
    assert_eq!(report["imported"][0]["records_imported"], json!(2));

    let client = app.client().await;
    let count: i64 = client
        .query_one("select count(*) from posts", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, 2, "the rows are back");

    app.cleanup().await;
}

#[tokio::test]
async fn the_import_endpoint_accepts_the_studio_options_and_reports_what_it_ignored() {
    let Some(app) = test_app("backup_http_options").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let archive = export(&app, &token).await;

    // Studio sends `deleteMissing` and `password` with every archive upload. The
    // server never deletes on import and the password confirms a rollback, so both
    // are accepted, and `deleteMissing` is reported back as ignored rather than
    // silently dropped or rejected.
    let reply = import(
        &app,
        &token,
        &archive,
        &[("deleteMissing", "true"), ("password", "correct-horse")],
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a report");
    assert_eq!(body["imported"][0]["name"], json!("posts"));
    assert_eq!(body["ignoredOptions"], json!(["deleteMissing"]));

    // A genuinely unknown option is still refused.
    let reply = import(&app, &token, &archive, &[("nonsense", "1")]).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let message = reply.body.expect("an error")["message"]
        .as_str()
        .expect("a message")
        .to_string();
    assert!(message.contains("nonsense"), "{message}");

    app.cleanup().await;
}

#[tokio::test]
async fn a_json_backup_can_be_imported_over_the_same_endpoint() {
    let Some(app) = test_app("backup_http_json").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // The single-document JSON `GET /api/export` produces, posted back as JSON — the
    // path Studio's JSON restore uses.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/export",
        None,
        &token,
    )
    .await;
    let payload = reply.body.expect("an export");
    sql(&app, "delete from posts").await;

    let body = json!({
        "collections": payload["collections"],
        "deleteMissing": true,
        "atomic": false,
        "password": "correct-horse",
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/import")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).expect("body")))
        .expect("request");
    let response = router(&app).await.oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("body");
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let report: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(report["ignoredOptions"], json!(["deleteMissing"]));

    let count: i64 = app
        .client()
        .await
        .query_one("select count(*) from posts", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, 2, "the JSON records are back");

    app.cleanup().await;
}

#[tokio::test]
async fn the_preflight_reports_what_it_would_gate_on() {
    let Some(app) = test_app("backup_http_preflight").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/import/preflight",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a preflight");
    assert_eq!(body["available"], json!(true));
    assert_eq!(body["confirm_records"], json!(10_000));
    assert!(body["collections"].is_number(), "{body}");

    app.cleanup().await;
}

#[tokio::test]
async fn a_zip_that_is_not_a_lazypock_archive_is_refused_with_a_reason() {
    let Some(app) = test_app("backup_http_foreign").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = import(&app, &token, b"definitely not a zip", &[]).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let message = reply.body.expect("an error")["message"]
        .as_str()
        .expect("a message")
        .to_string();
    assert!(message.contains("archive"), "{message}");

    app.cleanup().await;
}

/// Add a collection and a row that an import will then bring in, so a rollback has
/// something to remove.
async fn add_extra(app: &TestApp) {
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: "extra".into(),
            collection_type: CollectionType::Base,
            fields: vec![CreateField {
                name: "name".into(),
                field_type: FieldType::Text,
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
    .expect("create extra");
    app.reload_schema().await;
    sql(app, "insert into extra (id, name) values ('e1', 'one')").await;
    sql(app, "insert into posts (id, title) values ('p3', 'third')").await;
}

#[tokio::test]
async fn an_import_takes_a_snapshot_and_rollback_undoes_it() {
    let Some(app) = test_app("backup_rollback").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // No checkpoint before any import.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/import/status",
        None,
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert!(reply.body.expect("status")["snapshot"].is_null());

    // Build an archive that carries a collection and a row the import will add.
    add_extra(&app).await;
    let archive = export(&app, &token).await;

    // Put the database back to just `posts` p1/p2, so the import really adds.
    {
        let mut client = app.client().await;
        collections::delete(&mut client, "extra", true)
            .await
            .expect("drop extra");
    }
    app.reload_schema().await;
    sql(&app, "delete from posts where id = 'p3'").await;

    // Import: the snapshot is taken first, so it lacks `extra` and p3.
    let reply = import(&app, &token, &archive, &[("atomic", "batch")]).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    // The additions are there...
    let count: i64 = app
        .client()
        .await
        .query_one("select count(*) from extra", &[])
        .await
        .expect("extra")
        .get(0);
    assert_eq!(count, 1);

    // ...and the status reports the checkpoint.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/import/status",
        None,
        &token,
    )
    .await;
    let body = reply.body.expect("status");
    assert!(body["snapshot"]["created_at"].is_string(), "{body}");

    // Roll back, confirming with the password.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/import/rollback",
        Some(json!({ "password": "correct-horse" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("report")["rolled_back"], json!(true));

    // The collection the import created is gone, and so is the row it added.
    let exists: bool = app
        .client()
        .await
        .query_one("select to_regclass('extra') is not null", &[])
        .await
        .expect("check")
        .get(0);
    assert!(
        !exists,
        "a collection an import created must be removed by a rollback"
    );
    let count: i64 = app
        .client()
        .await
        .query_one("select count(*) from posts", &[])
        .await
        .expect("posts")
        .get(0);
    assert_eq!(
        count, 2,
        "a row an import added must be removed by a rollback"
    );

    // The checkpoint is spent.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/import/status",
        None,
        &token,
    )
    .await;
    assert!(reply.body.expect("status")["snapshot"].is_null());

    app.cleanup().await;
}

#[tokio::test]
async fn rolling_back_needs_a_password_and_a_snapshot() {
    let Some(app) = test_app("backup_rollback_gate").await else {
        return;
    };
    seed(&app).await;
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    // No checkpoint yet: a rollback is a 404 even with the right password.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/import/rollback",
        Some(json!({ "password": "correct-horse" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{:?}", reply.body);

    // Take a checkpoint by importing, then prove the password is required.
    let archive = export(&app, &token).await;
    let reply = import(&app, &token, &archive, &[("atomic", "batch")]).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/import/rollback",
        Some(json!({ "password": "wrong" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{:?}", reply.body);

    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/import/rollback",
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{:?}", reply.body);

    // A rejected rollback leaves the checkpoint alone.
    let reply = call_authed(
        &router(&app).await,
        Method::GET,
        "/api/import/status",
        None,
        &token,
    )
    .await;
    assert!(reply.body.expect("status")["snapshot"].is_object());

    app.cleanup().await;
}
