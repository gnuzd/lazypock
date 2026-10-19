//! `POST /api/mail`: application email through the configured SMTP.
//!
//! Sends are queued, so these tests read the `mail` job the endpoint enqueued rather
//! than trying to speak SMTP. Rendering and delivery are covered in `lazypock-mail`.

mod support;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::{json, Value};

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// Store a `mail` setting, as Studio → Settings → Mail would.
async fn configure_mail(app: &TestApp) {
    let client = app.client().await;
    client
        .execute(
            "insert into _settings (key, value) values ('mail', $1::text::jsonb) \
             on conflict (key) do update set value = excluded.value",
            &[
                &json!({ "host": "smtp.example", "sender_address": "noreply@example.com" })
                    .to_string(),
            ],
        )
        .await
        .expect("configure mail");
}

/// The most recently queued `mail` job's payload.
async fn latest_mail_job(app: &TestApp) -> Option<Value> {
    let client = app.client().await;
    client
        .query_opt(
            "select payload from _jobs where kind = 'mail' order by created desc limit 1",
            &[],
        )
        .await
        .expect("query jobs")
        .map(|row| row.get("payload"))
}

async fn send(app: &TestApp, token: &str, body: Value) -> support::Reply {
    call_authed(
        &router(app).await,
        Method::POST,
        "/api/mail",
        Some(body),
        token,
    )
    .await
}

#[tokio::test]
async fn a_templated_message_is_queued_for_the_configured_mailer() {
    let Some(app) = test_app("mail_template").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    configure_mail(&app).await;

    let reply = send(
        &app,
        &token,
        json!({
            "to": "ada@example.com",
            "toName": "Ada",
            "template": "verification",
            "assigns": { "token": "abc" },
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let body = reply.body.expect("a response");
    assert_eq!(body["queued"], json!(true));
    assert!(body["id"].as_str().is_some_and(|id| !id.is_empty()));

    let job = latest_mail_job(&app).await.expect("a queued job");
    assert_eq!(job["template"], json!("verification"));
    assert_eq!(job["to"], json!("ada@example.com"));
    assert_eq!(job["toName"], json!("Ada"));
    assert_eq!(job["assigns"]["token"], json!("abc"));

    app.cleanup().await;
}

#[tokio::test]
async fn a_raw_message_is_queued_with_its_parts() {
    let Some(app) = test_app("mail_raw").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    configure_mail(&app).await;

    let reply = send(
        &app,
        &token,
        json!({
            "to": "ada@example.com",
            "subject": "Your order shipped",
            "html": "<p>On the way</p>",
            "text": "On the way",
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    let job = latest_mail_job(&app).await.expect("a queued job");
    assert_eq!(job["to"], json!("ada@example.com"));
    assert_eq!(job["subject"], json!("Your order shipped"));
    assert_eq!(job["html"], json!("<p>On the way</p>"));
    assert_eq!(job["text"], json!("On the way"));
    // No template: this is the raw shape.
    assert!(job.get("template").is_none(), "{job}");

    app.cleanup().await;
}

#[tokio::test]
async fn an_api_key_can_queue_mail_but_an_anonymous_caller_cannot() {
    let Some(app) = test_app("mail_keys").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    configure_mail(&app).await;

    // An application server uses an API key, not a superuser password.
    let reply = call_authed(
        &router(&app).await,
        Method::POST,
        "/api/settings/api-keys",
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let key = reply.body.expect("a created key")["api_key"]
        .as_str()
        .expect("the raw key")
        .to_string();

    let reply = send(
        &app,
        &key,
        json!({ "to": "ada@example.com", "subject": "Hi", "text": "Hello" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    // No credential: this is not an open relay.
    let reply = call(
        &router(&app).await,
        Method::POST,
        "/api/mail",
        Some(json!({ "to": "ada@example.com", "subject": "Hi", "text": "Hello" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);

    app.cleanup().await;
}

#[tokio::test]
async fn a_message_that_cannot_be_sent_is_refused_with_a_reason() {
    let Some(app) = test_app("mail_validation").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    configure_mail(&app).await;

    let cases: &[(Value, &str)] = &[
        (json!({ "to": "ada@example.com" }), "template"),
        (
            json!({
                "to": "ada@example.com",
                "template": "verification",
                "subject": "Also a subject",
            }),
            "not both",
        ),
        (
            json!({ "to": "ada@example.com", "template": "receipt" }),
            "no mail template named",
        ),
        (
            json!({ "to": "not-an-address", "subject": "Hi", "text": "Hello" }),
            "email address",
        ),
        (
            json!({ "to": "ada@example.com", "subject": "", "text": "Hello" }),
            "subject",
        ),
        (json!({ "to": "ada@example.com", "subject": "Hi" }), "html"),
    ];

    for (body, expected) in cases {
        let reply = send(&app, &token, body.clone()).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{body}: {:?}",
            reply.body
        );
        let message = reply.body.expect("an error body")["message"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(
            message.contains(expected),
            "{body} produced {message:?}, expected {expected:?}"
        );
    }

    // Nothing was queued by a refused request.
    assert!(
        latest_mail_job(&app).await.is_none(),
        "a refused message must not leave a job behind"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn mail_that_is_not_configured_is_refused_before_queueing() {
    let Some(app) = test_app("mail_unconfigured").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;

    let reply = send(
        &app,
        &token,
        json!({ "to": "ada@example.com", "subject": "Hi", "text": "Hello" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{:?}", reply.body);
    let message = reply.body.expect("an error body")["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        message.contains("not configured") && message.contains("Settings → Mail"),
        "{message}"
    );
    assert!(
        latest_mail_job(&app).await.is_none(),
        "an unconfigured mailer must not queue a job that can only fail"
    );

    app.cleanup().await;
}
