//! `@request.auth.<field>` against a real database.
//!
//! A rule that reads a custom field of the authenticated record — PocketBase's
//! documented `@request.auth.role = 'admin'`, and the shape the file-rule docs use —
//! has to be true for an account whose `role` is `admin`. It was not: `auth.claim`
//! read a session setting nothing ever published, so every such rule compiled,
//! became a policy, and then evaluated to NULL for every request.

mod support;

use axum::http::{Method, StatusCode};
use serde_json::json;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use axum::Router;

use support::{call, call_authed, test_app, TestApp};

fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

fn field(name: &str, field_type: FieldType, options: serde_json::Value) -> CreateField {
    CreateField {
        name: name.into(),
        field_type,
        required: false,
        unique: false,
        options,
    }
}

async fn create(app: &TestApp, collection: CreateCollection) {
    let mut client = app.client().await;
    collections::create(&mut client, collection)
        .await
        .unwrap_or_else(|err| panic!("create {}: {err}", "collection"));
}

async fn login(app: &TestApp, email: &str) -> String {
    let reply = call(
        &router(app),
        Method::POST,
        "/api/collections/members/auth-with-password",
        Some(json!({ "identity": email, "password": "correct-horse" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    reply.body.expect("a login body")["token"]
        .as_str()
        .expect("a token")
        .to_string()
}

#[tokio::test]
async fn a_custom_field_on_the_authenticated_record_resolves_in_a_rule() {
    let Some(app) = test_app("auth_claims").await else {
        return;
    };
    let token = support::superuser_token(&app, "admin@example.com", "correct-horse").await;

    // An auth collection with a `role` field, and a base collection whose create rule
    // reads it — the shape the import format's examples use.
    create(
        &app,
        CreateCollection {
            name: "members".into(),
            collection_type: CollectionType::Auth,
            fields: vec![
                field("name", FieldType::Text, json!({})),
                field(
                    "role",
                    FieldType::Select,
                    json!({ "values": ["admin", "member"], "maxSelect": 1 }),
                ),
            ],
            rules: json!({ "createRule": "" }),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await;
    create(
        &app,
        CreateCollection {
            name: "notes".into(),
            collection_type: CollectionType::Base,
            fields: vec![field("title", FieldType::Text, json!({}))],
            rules: json!({
                "listRule": "",
                "createRule": "@request.auth.id != '' && @request.auth.role = 'admin'",
            }),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await;
    app.reload_schema().await;

    // Two accounts: one admin, one not. Registration is public on `members`.
    for (email, role) in [
        ("admin@member.test", "admin"),
        ("member@member.test", "member"),
    ] {
        let reply = call(
            &router(&app),
            Method::POST,
            "/api/collections/members/records",
            Some(json!({
                "email": email,
                "password": "correct-horse",
                "passwordConfirm": "correct-horse",
                "name": role,
                "role": role,
            })),
        )
        .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    }

    let admin = login(&app, "admin@member.test").await;
    let member = login(&app, "member@member.test").await;

    // The rule reads `@request.auth.role`, which is a custom field of the record.
    let reply = call_authed(
        &router(&app),
        Method::POST,
        "/api/collections/notes/records",
        Some(json!({ "title": "by a member" })),
        &member,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::FORBIDDEN,
        "a non-admin must not be allowed to create: {:?}",
        reply.body
    );

    let reply = call_authed(
        &router(&app),
        Method::POST,
        "/api/collections/notes/records",
        Some(json!({ "title": "by an admin" })),
        &admin,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    // The same reference in a *filter* (the query layer's context, not a policy).
    let reply = call_authed(
        &router(&app),
        Method::GET,
        "/api/collections/notes/records?filter=@request.auth.role%20%3D%20'admin'",
        None,
        &admin,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(
        reply.body.expect("a list")["totalItems"],
        json!(1),
        "an admin's filter on their own claim should match"
    );

    let reply = call_authed(
        &router(&app),
        Method::GET,
        "/api/collections/notes/records?filter=@request.auth.role%20%3D%20'admin'",
        None,
        &member,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(
        reply.body.expect("a list")["totalItems"],
        json!(0),
        "a member's filter on the admin claim should match nothing"
    );

    // Unauthenticated: the claim resolves to nothing, so the rule denies.
    let reply = call(
        &router(&app),
        Method::POST,
        "/api/collections/notes/records",
        Some(json!({ "title": "anon" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    // The superuser is unaffected: it runs as the owner and bypasses the rules.
    let reply = call_authed(
        &router(&app),
        Method::POST,
        "/api/collections/notes/records",
        Some(json!({ "title": "by a superuser" })),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    app.cleanup().await;
}
