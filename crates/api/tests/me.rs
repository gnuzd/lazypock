//! `GET /api/me`: the identity endpoint a client calls to learn who it is.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::json;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

#[tokio::test]
async fn me_requires_a_token() {
    let Some(app) = test_app("me_anonymous").await else {
        return;
    };

    let reply = call(&router(&app).await, Method::GET, "/api/me", None).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn me_reports_a_superuser_without_a_collection() {
    let Some(app) = test_app("me_superuser").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = call_authed(&router(&app).await, Method::GET, "/api/me", None, &token).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let body = reply.body.expect("an identity");
    assert_eq!(body["email"], json!("admin@example.com"));
    assert_eq!(body["superuser"], json!(true));
    assert!(body["id"].is_string(), "{body}");
    // A superuser belongs to no collection, and that absence is how a client
    // tells a superuser from a collection user.
    assert!(body.get("collectionName").is_none(), "{body}");
    assert!(body.get("collectionId").is_none(), "{body}");

    app.cleanup().await;
}

#[tokio::test]
async fn me_reports_a_collection_user_with_its_collection() {
    let Some(app) = test_app("me_user").await else {
        return;
    };

    // An auth collection with a public create rule, so a user can register.
    let mut client = app.client().await;
    collections::create(
        &mut client,
        CreateCollection {
            name: "members".into(),
            collection_type: CollectionType::Auth,
            fields: vec![CreateField {
                name: "name".into(),
                field_type: FieldType::Text,
                required: false,
                unique: false,
                options: json!({}),
            }],
            rules: json!({ "createRule": "", "listRule": "", "viewRule": "" }),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("create members");
    app.reload_schema().await;

    let reply = support::post(
        &router(&app).await,
        "/api/collections/members/records",
        json!({ "email": "member@example.com", "password": "correct-horse", "name": "Member" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);

    let reply = support::post(
        &router(&app).await,
        "/api/collections/members/auth-with-password",
        json!({ "identity": "member@example.com", "password": "correct-horse" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let token = reply.body.expect("a login")["token"]
        .as_str()
        .expect("token")
        .to_string();

    let reply = call_authed(&router(&app).await, Method::GET, "/api/me", None, &token).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let body = reply.body.expect("an identity");
    assert_eq!(body["email"], json!("member@example.com"));
    assert_eq!(body["superuser"], json!(false));
    assert_eq!(body["collectionName"], json!("members"));
    assert!(body["collectionId"].is_string(), "{body}");

    app.cleanup().await;
}
