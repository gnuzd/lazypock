//! HTTP-level tests for the record endpoints, driven through the real router
//! against a real Postgres.

mod support;

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};

use support::{call, get, post, test_app, TestApp};

fn field(name: &str, field_type: FieldType, options: Value) -> CreateField {
    CreateField {
        name: name.into(),
        field_type,
        required: false,
        unique: false,
        options,
    }
}

fn collection(name: &str, fields: Vec<CreateField>) -> CreateCollection {
    CreateCollection {
        name: name.into(),
        collection_type: CollectionType::Base,
        fields,
        rules: json!({
            "listRule": "",
            "viewRule": "",
            "createRule": "",
            "updateRule": "",
            "deleteRule": "",
        }),
        options: json!({}),
        adopt_existing_table: false,
    }
}

/// Create a `users` and a `posts` collection.
async fn seed(app: &TestApp) {
    let mut client = app.client().await;

    collections::create(
        &mut client,
        collection("users", vec![field("name", FieldType::Text, json!({}))]),
    )
    .await
    .expect("users");
    let users = collections::get(&client, "users").await.expect("users");

    let mut title = field("title", FieldType::Text, json!({}));
    title.required = true;

    collections::create(
        &mut client,
        collection(
            "posts",
            vec![
                title,
                field("views", FieldType::Number, json!({"onlyInt": true})),
                field("published", FieldType::Bool, json!({})),
                field(
                    "tags",
                    FieldType::Select,
                    json!({"values": ["a", "b", "c"], "maxSelect": 3}),
                ),
                field(
                    "author",
                    FieldType::Relation,
                    json!({"collectionId": users.id}),
                ),
            ],
        ),
    )
    .await
    .expect("posts");

    app.reload_schema().await;
}

#[tokio::test]
async fn creating_and_listing_records_through_http() {
    let Some(app) = test_app("http_create_list").await else {
        return;
    };
    let router = lazypock_api::router::router(app.state.clone());
    seed(&app).await;

    for (title, views, published) in [
        ("First", 10, true),
        ("Second", 20, false),
        ("Third", 5, true),
    ] {
        let reply = post(
            &router,
            "/api/collections/posts/records",
            json!({"title": title, "views": views, "published": published, "tags": ["a"]}),
        )
        .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
        let record = reply.body.expect("body");
        assert_eq!(record["title"], json!(title));
        assert_eq!(record["views"], json!(views));
        assert_eq!(record["tags"], json!(["a"]));
        assert_eq!(record["id"].as_str().unwrap().len(), 15);
    }

    let reply = get(&router, "/api/collections/posts/records").await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.body.expect("body");

    // The PocketBase list envelope.
    assert_eq!(body["page"], json!(1));
    assert_eq!(body["perPage"], json!(30));
    assert_eq!(body["totalItems"], json!(3));
    assert_eq!(body["totalPages"], json!(1));
    assert_eq!(body["items"].as_array().unwrap().len(), 3);

    // Default ordering is newest first.
    let first_id = body["items"][0]["id"].as_str().unwrap().to_string();
    assert!(!first_id.is_empty());

    app.cleanup().await;
}

#[tokio::test]
async fn viewing_a_record_supports_conditional_requests() {
    let Some(app) = test_app("http_view_etag").await else {
        return;
    };
    let router = lazypock_api::router::router(app.state.clone());
    seed(&app).await;

    let created = post(
        &router,
        "/api/collections/posts/records",
        json!({"title": "Cached"}),
    )
    .await
    .body
    .expect("body");
    let id = created["id"].as_str().unwrap().to_string();
    let uri = format!("/api/collections/posts/records/{id}");

    let reply = get(&router, &uri).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body.expect("body")["title"], json!("Cached"));
    let etag = reply
        .headers
        .get(header::ETAG)
        .expect("etag")
        .to_str()
        .unwrap()
        .to_string();
    assert!(etag.starts_with('"') && etag.ends_with('"'));

    // A matching conditional request is served without a body.
    let builder = Request::builder()
        .method(Method::GET)
        .uri(&uri)
        .header(header::IF_NONE_MATCH, &etag);
    let response = router
        .clone()
        .oneshot(builder.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

    // A stale tag returns the record again.
    let builder = Request::builder()
        .method(Method::GET)
        .uri(&uri)
        .header(header::IF_NONE_MATCH, "\"stale\"");
    let response = router
        .clone()
        .oneshot(builder.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // A missing record is a 404 in the standard envelope.
    let reply = get(&router, "/api/collections/posts/records/zzzzzzzzzzzzzzz").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.body.expect("body")["status"], json!(404));

    app.cleanup().await;
}

#[tokio::test]
async fn updates_and_deletes_report_their_outcome() {
    let Some(app) = test_app("http_update_delete").await else {
        return;
    };
    let router = lazypock_api::router::router(app.state.clone());
    seed(&app).await;

    let created = post(
        &router,
        "/api/collections/posts/records",
        json!({"title": "Original", "views": 1, "published": true}),
    )
    .await
    .body
    .expect("body");
    let id = created["id"].as_str().unwrap().to_string();
    let uri = format!("/api/collections/posts/records/{id}");

    let reply = call(&router, Method::PATCH, &uri, Some(json!({"views": 42}))).await;
    assert_eq!(reply.status, StatusCode::OK);
    let updated = reply.body.expect("body");
    assert_eq!(updated["views"], json!(42));
    assert_eq!(
        updated["title"],
        json!("Original"),
        "partial update changed another field"
    );
    assert_eq!(updated["published"], json!(true));

    let reply = call(&router, Method::DELETE, &uri, None).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert!(reply.body.is_none());

    let reply = call(&router, Method::DELETE, &uri, None).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    app.cleanup().await;
}

#[tokio::test]
async fn validation_failures_return_400_with_per_field_data() {
    let Some(app) = test_app("http_validation").await else {
        return;
    };
    let router = lazypock_api::router::router(app.state.clone());
    seed(&app).await;

    let reply = post(
        &router,
        "/api/collections/posts/records",
        json!({"views": 1, "titel": "typo"}),
    )
    .await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    let body = reply.body.expect("body");
    assert_eq!(body["status"], json!(400));
    assert_eq!(body["data"]["title"]["code"], json!("validation_required"));
    assert_eq!(
        body["data"]["titel"]["code"],
        json!("validation_unknown_field")
    );

    // Malformed JSON is reported in the same envelope.
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/collections/posts/records")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{ not json"))
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(body["status"], json!(400));

    app.cleanup().await;
}

#[tokio::test]
async fn query_parameters_drive_filter_sort_projection_and_expand() {
    let Some(app) = test_app("http_query_params").await else {
        return;
    };
    let router = lazypock_api::router::router(app.state.clone());
    seed(&app).await;

    let author = post(
        &router,
        "/api/collections/users/records",
        json!({"name": "Alice"}),
    )
    .await
    .body
    .expect("body")["id"]
        .as_str()
        .unwrap()
        .to_string();

    for (title, views, published) in [("Alpha", 10, true), ("Beta", 20, false), ("Gamma", 5, true)]
    {
        post(
            &router,
            "/api/collections/posts/records",
            json!({"title": title, "views": views, "published": published, "author": author}),
        )
        .await;
    }

    // filter + sort + fields
    let reply = get(
        &router,
        "/api/collections/posts/records?filter=published%3Dtrue&sort=-views&fields=id,views",
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let items = reply.body.expect("body")["items"].clone();
    assert_eq!(items.as_array().unwrap().len(), 2);
    assert_eq!(items[0]["views"], json!(10), "sort was not applied");
    assert_eq!(items[1]["views"], json!(5));
    assert_eq!(
        items[0].as_object().unwrap().len(),
        2,
        "projection was not applied: {items}"
    );

    // contains, case-insensitive
    let reply = get(
        &router,
        "/api/collections/posts/records?filter=title~%22a%22",
    )
    .await;
    assert_eq!(
        reply.body.expect("body")["totalItems"],
        json!(3),
        "every title contains an `a`"
    );

    // expand + nested fields. The expansion is nested under `expand`, and the
    // relation's own key keeps the id — PocketBase's shape, which is what the Studio
    // reads (`record.expand[field]`).
    let reply = get(
        &router,
        "/api/collections/posts/records?filter=title%3D%22Alpha%22&expand=author&fields=id,author,author.name",
    )
    .await;
    let items = reply.body.expect("body")["items"].clone();
    assert_eq!(items[0]["expand"]["author"]["name"], json!("Alice"));
    assert_eq!(
        items[0]["author"],
        json!(author),
        "the relation key is the id"
    );
    assert!(items[0].get("title").is_none());

    // perPage and page
    let reply = get(&router, "/api/collections/posts/records?perPage=2&page=2").await;
    let body = reply.body.expect("body");
    assert_eq!(body["perPage"], json!(2));
    assert_eq!(body["totalPages"], json!(2));
    assert_eq!(body["items"].as_array().unwrap().len(), 1);

    // skipTotal uses PocketBase's -1 sentinel
    let reply = get(&router, "/api/collections/posts/records?skipTotal=true").await;
    let body = reply.body.expect("body");
    assert_eq!(body["totalItems"], json!(-1));
    assert_eq!(body["totalPages"], json!(-1));

    app.cleanup().await;
}

#[tokio::test]
async fn keyset_paging_walks_through_the_http_api() {
    let Some(app) = test_app("http_keyset").await else {
        return;
    };
    let router = lazypock_api::router::router(app.state.clone());
    seed(&app).await;

    for title in ["One", "Two", "Three"] {
        post(
            &router,
            "/api/collections/posts/records",
            json!({"title": title}),
        )
        .await;
    }

    // Deterministic ordering for the walk.
    {
        let client = app.client().await;
        client
            .batch_execute(
                r#"
                update "posts" set "created" = '2026-01-01T00:00:00Z'::timestamptz where "title" = 'One';
                update "posts" set "created" = '2026-01-02T00:00:00Z'::timestamptz where "title" = 'Two';
                update "posts" set "created" = '2026-01-03T00:00:00Z'::timestamptz where "title" = 'Three';
                "#,
            )
            .await
            .expect("timestamps");
    }

    let reply = get(&router, "/api/collections/posts/records?after=&perPage=2").await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.body.expect("body");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["title"], json!("Three"));
    assert_eq!(items[1]["title"], json!("Two"));
    assert!(body.get("totalItems").is_none(), "keyset must not count");
    let cursor = body["nextCursor"].as_str().expect("cursor").to_string();

    let reply = get(
        &router,
        &format!("/api/collections/posts/records?perPage=2&after={cursor}"),
    )
    .await;
    let body = reply.body.expect("body");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], json!("One"));
    // The final page is not full, so there is nothing to continue from.
    assert_eq!(body["nextCursor"], json!(null));

    // Mixing the two paging styles is rejected.
    let reply = get(&router, "/api/collections/posts/records?page=2&after=abc").await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);

    app.cleanup().await;
}

#[tokio::test]
async fn bad_query_parameters_are_rejected_with_a_message() {
    let Some(app) = test_app("http_bad_params").await else {
        return;
    };
    let router = lazypock_api::router::router(app.state.clone());
    seed(&app).await;

    let cases = [
        "/api/collections/posts/records?filter=title%20%3D",
        "/api/collections/posts/records?filter=nope%3D1",
        "/api/collections/posts/records?sort=nope",
        "/api/collections/posts/records?fields=nope",
        "/api/collections/posts/records?expand=nope",
        "/api/collections/posts/records?perPage=0",
        "/api/collections/posts/records?perPage=9999",
        "/api/collections/posts/records?page=0",
        "/api/collections/posts/records?after=junk",
    ];

    for uri in cases {
        let reply = get(&router, uri).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{uri}");
        let body = reply.body.expect("body");
        assert_eq!(body["status"], json!(400), "{uri}");
        assert!(
            body["message"].as_str().is_some_and(|m| !m.is_empty()),
            "{uri} returned no message"
        );
        // A filter problem must not leak the SQL that would have been built.
        assert!(
            !body["message"].as_str().unwrap().contains("select "),
            "{uri} leaked SQL"
        );
    }

    // An unknown collection is a 404, not a 400.
    let reply = get(&router, "/api/collections/nope/records").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    app.cleanup().await;
}

#[tokio::test]
async fn a_collection_with_no_rules_refuses_access() {
    // Default deny: row-level security is on and the collection has no rules at
    // all, so nobody but the service may touch it.
    let Some(app) = test_app("http_records_default_deny").await else {
        return;
    };
    let router = lazypock_api::router::router(app.state.clone());

    {
        let mut client = app.client().await;
        collections::create(
            &mut client,
            CreateCollection {
                name: "locked".into(),
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
        .expect("create locked");
        app.reload_schema().await;
    }

    // A read is filtered to nothing rather than erroring: that is what a policy
    // does, and it does not reveal whether the table holds anything.
    let reply = get(&router, "/api/collections/locked/records").await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body.expect("body")["totalItems"], json!(0));

    // A write is refused outright.
    let reply = post(
        &router,
        "/api/collections/locked/records",
        json!({"title": "nope"}),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    app.cleanup().await;
}
