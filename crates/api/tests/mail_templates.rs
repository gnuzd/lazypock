//! Mail templates stored in the database.
//!
//! The failures these cover are the quiet ones: a body that is saved but never
//! rendered, a body that is stored without being parsed, a template that cannot be
//! reset, and a table a request can still reach.

mod support;

use std::collections::BTreeMap;

use axum::http::{Method, StatusCode};
use axum::Router;
use serde_json::{json, Value};

use support::{call, call_authed, superuser_token, test_app, TestApp};

async fn router(app: &TestApp) -> Router {
    lazypock_api::router::router(app.state.clone())
}

/// The three parts, as `PUT` wants them.
fn parts(subject: &str, html: &str, text: &str) -> Value {
    json!({ "subject": subject, "html": html, "text": text })
}

/// The listed templates, keyed by name.
fn by_name(reply: &Value) -> BTreeMap<String, Value> {
    reply["items"]
        .as_array()
        .expect("an items array")
        .iter()
        .map(|item| {
            (
                item["name"].as_str().expect("a name").to_string(),
                item.clone(),
            )
        })
        .collect()
}

#[tokio::test]
async fn the_built_in_templates_are_listed_with_the_body_and_the_layer_they_come_from() {
    let Some(app) = test_app("mail_templates_list").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let api = router(&app).await;

    let reply = call_authed(&api, Method::GET, "/api/mail/templates", None, &token).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let items = by_name(&reply.body.expect("a list"));

    for name in ["verification", "password_reset", "email_change", "otp"] {
        let template = items
            .get(name)
            .unwrap_or_else(|| panic!("{name} should be listed"));
        assert_eq!(template["source"], json!("builtin"), "{name}");
        assert_eq!(template["parts"]["subject"], json!("builtin"), "{name}");
        assert_eq!(template["parts"]["html"], json!("builtin"), "{name}");
        assert_eq!(template["parts"]["text"], json!("builtin"), "{name}");
        // The *body* rather than a rendered message: the editor needs the source.
        let subject = template["subject"].as_str().expect("a subject");
        assert!(subject.contains("{{"), "{name}: {subject}");
    }

    app.cleanup().await;
}

#[tokio::test]
async fn a_saved_body_wins_and_deleting_it_gives_the_built_in_back() {
    let Some(app) = test_app("mail_templates_save").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let api = router(&app).await;

    let saved = call_authed(
        &api,
        Method::PUT,
        "/api/mail/templates/otp",
        Some(parts("Code {{ code }}", "<p>{{ code }}</p>", "{{ code }}")),
        &token,
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{:?}", saved.body);
    let saved = saved.body.expect("a template");
    assert_eq!(saved["source"], json!("database"));
    assert_eq!(saved["parts"]["subject"], json!("database"));
    assert_eq!(saved["subject"], json!("Code {{ code }}"));

    // The list agrees, and the templates next to it are untouched.
    let listed = call_authed(&api, Method::GET, "/api/mail/templates", None, &token).await;
    let items = by_name(&listed.body.expect("a list"));
    assert_eq!(items["otp"]["source"], json!("database"));
    assert_eq!(items["verification"]["source"], json!("builtin"));

    // The preview renders the saved body, through the same path a send uses.
    let preview = call_authed(
        &api,
        Method::POST,
        "/api/mail/templates/otp/preview",
        Some(json!({ "assigns": { "code": "123456", "name": "Ada" } })),
        &token,
    )
    .await;
    assert_eq!(preview.status, StatusCode::OK, "{:?}", preview.body);
    let rendered = preview.body.expect("a rendered message");
    assert_eq!(rendered["subject"], json!("Code 123456"));
    assert_eq!(rendered["html"], json!("<p>123456</p>"));
    assert_eq!(rendered["text"], json!("123456"));

    // Deleting removes the stored bodies rather than leaving a copy of the built-in.
    let deleted = call_authed(
        &api,
        Method::DELETE,
        "/api/mail/templates/otp",
        None,
        &token,
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK, "{:?}", deleted.body);
    assert_eq!(deleted.body.expect("a result")["deleted"], json!(3));

    let after = call_authed(&api, Method::GET, "/api/mail/templates/otp", None, &token).await;
    assert_eq!(after.status, StatusCode::OK);
    assert_eq!(after.body.expect("a template")["source"], json!("builtin"));

    // Deleting one that is already at its default is a no-op, not an error.
    let again = call_authed(
        &api,
        Method::DELETE,
        "/api/mail/templates/otp",
        None,
        &token,
    )
    .await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.body.expect("a result")["deleted"], json!(0));

    // A name nothing knows is a 404, so a typo cannot look like a reset.
    let unknown = call_authed(
        &api,
        Method::DELETE,
        "/api/mail/templates/nope",
        None,
        &token,
    )
    .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{:?}", unknown.body);

    app.cleanup().await;
}

#[tokio::test]
async fn a_body_is_parsed_before_it_is_stored() {
    let Some(app) = test_app("mail_templates_parse").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let api = router(&app).await;

    // A template with a syntax error would otherwise be stored happily and fail on a
    // queued email, where nobody is watching for it.
    let bad = call_authed(
        &api,
        Method::PUT,
        "/api/mail/templates/otp",
        Some(parts("{{ code }}", "{% if code %}unclosed", "{{ code }}")),
        &token,
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{:?}", bad.body);

    // And nothing was stored, so the built-in is still what renders.
    let after = call_authed(&api, Method::GET, "/api/mail/templates/otp", None, &token).await;
    assert_eq!(after.body.expect("a template")["source"], json!("builtin"));

    // An empty part is refused rather than stored.
    let empty = call_authed(
        &api,
        Method::PUT,
        "/api/mail/templates/otp",
        Some(parts("{{ code }}", "", "{{ code }}")),
        &token,
    )
    .await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST, "{:?}", empty.body);

    // A template arrives complete or not at all.
    let incomplete = call_authed(
        &api,
        Method::PUT,
        "/api/mail/templates/otp",
        Some(json!({ "subject": "{{ code }}", "html": "<p>{{ code }}</p>" })),
        &token,
    )
    .await;
    assert_eq!(
        incomplete.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        incomplete.body
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_preview_renders_a_draft_without_saving_it() {
    let Some(app) = test_app("mail_templates_preview").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let api = router(&app).await;

    // A draft part is rendered in place of the stored one — which is the point: a
    // body never has to be saved to be seen. The parts it does not name still come
    // from the built-in.
    let preview = call_authed(
        &api,
        Method::POST,
        "/api/mail/templates/otp/preview",
        Some(json!({
            "assigns": { "code": "1234", "name": "Ada", "expires_minutes": 5 },
            "subject": "Draft code {{ code }}",
        })),
        &token,
    )
    .await;
    assert_eq!(preview.status, StatusCode::OK, "{:?}", preview.body);
    let rendered = preview.body.expect("a rendered message");
    assert_eq!(rendered["subject"], json!("Draft code 1234"));
    assert!(rendered["text"].as_str().expect("text").contains("1234"));

    // Nothing was stored by rendering it.
    let stored = call_authed(&api, Method::GET, "/api/mail/templates/otp", None, &token).await;
    assert_eq!(stored.body.expect("a template")["source"], json!("builtin"));

    // An undefined variable is the caller's missing `assigns`, reported as such
    // rather than as a server fault: strict rendering is what makes an OTP email with
    // an empty code impossible, and the message is what says which key is missing.
    let missing = call_authed(
        &api,
        Method::POST,
        "/api/mail/templates/otp/preview",
        Some(json!({ "assigns": { "name": "Ada" } })),
        &token,
    )
    .await;
    assert_eq!(
        missing.status,
        StatusCode::BAD_REQUEST,
        "{:?}",
        missing.body
    );

    app.cleanup().await;
}

#[tokio::test]
async fn a_saved_template_adds_a_name_the_send_path_accepts() {
    let Some(app) = test_app("mail_templates_new_name").await else {
        return;
    };
    let token = superuser_token(&app, "admin@example.com", "correct-horse").await;
    let api = router(&app).await;

    // A name with nothing beneath it cannot be previewed from one part: it would be a
    // template missing a body, which is the thing the mailer refuses to hold.
    let partial = call_authed(
        &api,
        Method::POST,
        "/api/mail/templates/receipt/preview",
        Some(json!({ "subject": "Your receipt" })),
        &token,
    )
    .await;
    assert_eq!(
        partial.status,
        StatusCode::BAD_REQUEST,
        "{:?}",
        partial.body
    );

    let saved = call_authed(
        &api,
        Method::PUT,
        "/api/mail/templates/receipt",
        Some(parts(
            "Your receipt",
            "<p>Thanks {{ name }}</p>",
            "Thanks {{ name }}",
        )),
        &token,
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{:?}", saved.body);
    assert_eq!(saved.body.expect("a template")["source"], json!("database"));

    // It is listed...
    let listed = call_authed(&api, Method::GET, "/api/mail/templates", None, &token).await;
    assert!(by_name(&listed.body.expect("a list")).contains_key("receipt"));

    // ...and a send can name it. Mail is not configured in this test, so a send that
    // gets *past* the name is refused for that reason instead — which is what makes
    // this an assertion about the name rather than about SMTP.
    let send = call_authed(
        &api,
        Method::POST,
        "/api/mail",
        Some(json!({
            "to": "ada@example.com",
            "template": "receipt",
            "assigns": { "name": "Ada" },
        })),
        &token,
    )
    .await;
    assert_eq!(send.status, StatusCode::BAD_REQUEST, "{:?}", send.body);
    let message = send.body.expect("an error").to_string();
    assert!(message.contains("not configured"), "{message}");

    // An unknown name is refused by listing what is known — and the saved name is one
    // of them, so a template defined through the API is genuinely sendable.
    let unknown = call_authed(
        &api,
        Method::POST,
        "/api/mail",
        Some(json!({ "to": "ada@example.com", "template": "nope" })),
        &token,
    )
    .await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    let message = unknown.body.expect("an error").to_string();
    assert!(message.contains("receipt"), "{message}");

    app.cleanup().await;
}

#[tokio::test]
async fn defining_a_template_needs_a_superuser_and_never_an_api_key() {
    let Some(app) = test_app("mail_templates_authz").await else {
        return;
    };
    let api = router(&app).await;

    // A template is rendered with the variables of an auth email — a verification or
    // password-reset token, the recipient's address — so whoever can write one can
    // mail themselves somebody else's token.
    for (method, uri, body) in [
        (Method::GET, "/api/mail/templates", None),
        (
            Method::PUT,
            "/api/mail/templates/otp",
            Some(parts("a", "b", "c")),
        ),
        (Method::DELETE, "/api/mail/templates/otp", None),
        (
            Method::POST,
            "/api/mail/templates/otp/preview",
            Some(json!({ "assigns": {} })),
        ),
    ] {
        let reply = call(&api, method, uri, body).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{uri} must need a superuser: {:?} {}",
            reply.status,
            reply.body.map(|body| body.to_string()).unwrap_or_default()
        );
    }

    app.cleanup().await;
}

#[tokio::test]
async fn the_restricted_roles_cannot_reach_the_template_table() {
    let Some(app) = test_app("mail_templates_privileges").await else {
        return;
    };
    let client = app.client().await;

    // `0003_rls.sql` sets default privileges that grant every table created afterwards
    // to both request roles, so this table arrives readable and writable by a
    // request unless the migration revokes that. A body a request can write is a body
    // it can put a template in that mails somebody else's token to itself.
    for role in ["lazypock_anon", "lazypock_authenticated"] {
        for privilege in ["SELECT", "INSERT", "UPDATE", "DELETE"] {
            let granted: bool = client
                .query_one(
                    "select has_table_privilege($1::name, $2::text, $3::text)",
                    &[&role, &"_mail_templates", &privilege],
                )
                .await
                .expect("privilege lookup")
                .get(0);
            assert!(
                !granted,
                "{role} must not hold {privilege} on _mail_templates"
            );
        }
    }

    app.cleanup().await;
}
