//! Mail templates over the API: `GET/PUT/DELETE /api/mail/templates`.
//!
//! A deployment could already change an email by dropping a file in the template
//! directory, which needs a deploy, so a typo in a password-reset email meant a
//! rebuild. These endpoints are the layer that does not: a body saved here is
//! stored in `_mail_templates` and wins over the file, which wins over the built-in.
//!
//! Three things are deliberate:
//!
//! * **Superuser only, never an API key.** A template is rendered with the variables
//!   of an auth email — a verification or password-reset token, the recipient's
//!   address — so a key that could write a template could mail itself somebody
//!   else's token. Sending an already-defined template stays API-keyable; defining
//!   one does not.
//! * **A body is parsed before it is stored.** The mailer refuses a bad template at
//!   boot, and a save that accepted one would move that failure to a queued job on an
//!   email nobody is watching.
//! * **Nothing is seeded.** A template only exists in the database once it is saved
//!   here, so `DELETE` means "back to the file or the built-in" rather than "back to
//!   a copy of the built-in taken when this deployment was installed".
//!
//! The response reports, per part, which layer the body came from
//! (`database`/`file`/`builtin`), because an edit that is not in effect and an edit
//! that is look identical otherwise.

use axum::extract::{Json, Path, State};
use axum::response::Json as ResponseJson;
use lazypock_core::error::Error;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::audit;
use crate::auth::Superuser;
use crate::error::ApiError;
use crate::jobs;
use crate::state::AppState;

/// `GET /api/mail/templates`
///
/// Every template the server can send, with the body each part currently resolves to
/// and the layer it came from.
pub async fn list(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<ResponseJson<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let templates = effective(&client, &state.mail_templates).await?;
    let items: Vec<Value> = templates
        .names()
        .into_iter()
        .filter_map(|name| entry(&templates, name))
        .collect();
    Ok(ResponseJson(json!({ "items": items })))
}

/// `GET /api/mail/templates/{name}`
pub async fn get(
    State(state): State<AppState>,
    Path(name): Path<String>,
    _: Superuser,
) -> Result<ResponseJson<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let templates = effective(&client, &state.mail_templates).await?;
    entry(&templates, name.trim())
        .map(ResponseJson)
        .ok_or_else(|| not_found(name.trim(), &templates))
}

/// The body of `PUT /api/mail/templates/{name}`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveTemplate {
    /// The subject, as a template.
    pub subject: String,
    /// The HTML body.
    pub html: String,
    /// The plain-text alternative.
    pub text: String,
}

/// `PUT /api/mail/templates/{name}`
///
/// Saves all three parts, replacing whatever was stored for them.
///
/// All three rather than one at a time: a template is only allowed to exist once it
/// is complete, and a part-at-a-time write would let a subject be saved against a
/// body that fails to parse.
pub async fn save(
    State(state): State<AppState>,
    Path(name): Path<String>,
    user: Superuser,
    Json(body): Json<SaveTemplate>,
) -> Result<ResponseJson<Value>, ApiError> {
    let name = name.trim();
    validate_name(name)?;

    let parts = [
        ("subject", body.subject.as_str()),
        ("html", body.html.as_str()),
        ("text", body.text.as_str()),
    ];
    for (part, text) in parts {
        if text.trim().is_empty() {
            return Err(invalid(format!("`{part}` must not be empty")));
        }
        if text.len() > lazypock_db::mail_templates::MAX_BODY_BYTES {
            return Err(invalid(format!(
                "`{part}` is limited to {} bytes",
                lazypock_db::mail_templates::MAX_BODY_BYTES
            )));
        }
    }
    // Parse now rather than at send time: a template with a syntax error would
    // otherwise be stored happily and fail on a queued email, where nobody is
    // watching for it.
    lazypock_mail::Templates::check(name, &parts).map_err(ApiError::new)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    lazypock_db::mail_templates::save(&client, name, &parts)
        .await
        .map_err(ApiError::new)?;

    // Which parts, never the bodies: an audit entry is not the place to copy an email
    // body, and a template can be 64 KiB.
    audit::record(
        &client,
        &user.id,
        audit::action::MAIL_TEMPLATE_SAVE,
        Some(name),
        json!({ "parts": ["subject", "html", "text"] }),
    )
    .await
    .map_err(ApiError::new)?;

    let templates = effective(&client, &state.mail_templates).await?;
    entry(&templates, name)
        .map(ResponseJson)
        .ok_or_else(|| not_found(name, &templates))
}

/// `DELETE /api/mail/templates/{name}`
///
/// Removes the stored bodies, so the template goes back to the file or the built-in.
/// Deleting one that was never saved is a success with `deleted: 0` — it is already
/// at its default — but an unknown *name* is a 404, so a typo cannot look like a
/// reset that worked.
pub async fn delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
    user: Superuser,
) -> Result<ResponseJson<Value>, ApiError> {
    let name = name.trim();
    validate_name(name)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    let known = effective(&client, &state.mail_templates).await?;
    if known.template(name).is_none() {
        return Err(not_found(name, &known));
    }

    let removed = lazypock_db::mail_templates::delete(&client, name)
        .await
        .map_err(ApiError::new)?;

    audit::record(
        &client,
        &user.id,
        audit::action::MAIL_TEMPLATE_DELETE,
        Some(name),
        json!({ "parts": removed }),
    )
    .await
    .map_err(ApiError::new)?;

    Ok(ResponseJson(json!({ "name": name, "deleted": removed })))
}

/// The body of `POST /api/mail/templates/{name}/preview`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewTemplate {
    /// The template context. `name` and `app_name` are filled in when absent.
    #[serde(default)]
    pub assigns: Option<Value>,
    /// The recipient display name that `name` renders as.
    #[serde(default)]
    pub to_name: Option<String>,
    /// A draft subject to render instead of the stored one.
    #[serde(default)]
    pub subject: Option<String>,
    /// A draft HTML body to render instead of the stored one.
    #[serde(default)]
    pub html: Option<String>,
    /// A draft text body to render instead of the stored one.
    #[serde(default)]
    pub text: Option<String>,
}

/// `POST /api/mail/templates/{name}/preview`
///
/// Renders the template the way a send would, so an editor can show what will arrive
/// rather than what the source says it will. Any of `subject`/`html`/`text` that is
/// given is rendered in place of the stored part, which is what makes this useful
/// while a body is still being written: it never has to be saved to be seen, and
/// nothing is stored or sent.
pub async fn preview(
    State(state): State<AppState>,
    Path(name): Path<String>,
    _: Superuser,
    Json(body): Json<PreviewTemplate>,
) -> Result<ResponseJson<Value>, ApiError> {
    let name = name.trim();
    validate_name(name)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    let templates = effective(&client, &state.mail_templates).await?;

    let drafts: Vec<(&str, &str)> = [
        ("subject", body.subject.as_deref()),
        ("html", body.html.as_deref()),
        ("text", body.text.as_deref()),
    ]
    .into_iter()
    .filter_map(|(part, draft)| draft.map(|draft| (part, draft)))
    .collect();

    // A draft over an unknown name is *previewed* as the new template it is being
    // written as, so it has to arrive complete: saying that here keeps the mail crate's
    // "a template is complete or it does not exist" rule from surfacing as a server
    // error. With no draft there is nothing to preview, which is a 404.
    let known = templates.template(name).is_some();
    if !known && drafts.is_empty() {
        return Err(not_found(name, &templates));
    }
    if !known && drafts.len() < lazypock_db::mail_templates::PARTS.len() {
        return Err(invalid(format!(
            "there is no mail template named {name:?} yet, so all three parts are needed \
             to preview it (subject, html, text)"
        )));
    }
    if !drafts.is_empty() {
        for (part, draft) in &drafts {
            if draft.len() > lazypock_db::mail_templates::MAX_BODY_BYTES {
                return Err(invalid(format!(
                    "`{part}` is limited to {} bytes",
                    lazypock_db::mail_templates::MAX_BODY_BYTES
                )));
            }
        }
        // A draft that does not parse is the caller's input, not a server fault.
        lazypock_mail::Templates::check(name, &drafts).map_err(ApiError::new)?;
    }

    let overrides: Vec<lazypock_mail::Override> = drafts
        .iter()
        .map(|(part, draft)| lazypock_mail::Override {
            name: name.to_string(),
            part: (*part).to_string(),
            body: (*draft).to_string(),
        })
        .collect();
    let templates = templates
        .with_overrides(&overrides)
        .map_err(ApiError::new)?;

    let settings = jobs::read_settings(&client)
        .await
        .map_err(|err| ApiError::new(Error::internal(err)))?;
    let app_name = settings
        .get("app_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("lazypock");

    // The context a send builds, through the same function, so a preview cannot show
    // something a send would render differently.
    let assigns = body.assigns.unwrap_or(Value::Null);
    let context = lazypock_mail::context_for(
        body.to_name.as_deref().unwrap_or_default(),
        app_name,
        &assigns,
    )
    .map_err(ApiError::new)?;

    let rendered = templates.render_all(name, &context).map_err(|err| {
        // An undefined variable is a missing `assigns` key, which is the caller's
        // input rather than a server fault — and the message is what the editor shows
        // to say which one is missing.
        invalid(err.to_string())
    })?;

    Ok(ResponseJson(json!({
        "name": name,
        "subject": rendered.subject,
        "html": rendered.html,
        "text": rendered.text,
    })))
}

/// The templates with the database's bodies applied over the files and built-ins.
///
/// Shared by the endpoints here and by the send paths, so a template is resolved the
/// same way whether it is being listed, previewed or sent.
pub(crate) async fn effective(
    client: &lazypock_db::deadpool_postgres::Client,
    base: &lazypock_mail::Templates,
) -> Result<lazypock_mail::Templates, ApiError> {
    let stored = lazypock_db::mail_templates::load(client)
        .await
        .map_err(ApiError::new)?;
    let overrides: Vec<lazypock_mail::Override> = stored
        .iter()
        .map(|row| lazypock_mail::Override {
            name: row.name.clone(),
            part: row.part.clone(),
            body: row.body.clone(),
        })
        .collect();
    base.with_overrides(&overrides).map_err(ApiError::new)
}

/// One template as the API describes it: the resolved bodies, the layer each came
/// from, and the strongest layer overall so a list can badge it without walking them.
fn entry(templates: &lazypock_mail::Templates, name: &str) -> Option<Value> {
    let parts = templates.template(name)?;

    let mut bodies = Map::new();
    let mut sources = Map::new();
    for resolved in &parts {
        bodies.insert(resolved.part.to_string(), json!(resolved.body));
        sources.insert(resolved.part.to_string(), json!(resolved.origin.as_str()));
    }

    let source = parts
        .iter()
        .map(|resolved| resolved.origin)
        .min_by_key(|origin| rank(*origin))?;

    Some(json!({
        "name": name,
        "source": source.as_str(),
        "parts": sources,
        "subject": bodies.get("subject").cloned().unwrap_or(Value::Null),
        "html": bodies.get("html").cloned().unwrap_or(Value::Null),
        "text": bodies.get("text").cloned().unwrap_or(Value::Null),
    }))
}

/// Newest layer first, so the strongest layer a template has is the smallest rank.
fn rank(origin: lazypock_mail::Origin) -> u8 {
    match origin {
        lazypock_mail::Origin::Database => 0,
        lazypock_mail::Origin::File => 1,
        lazypock_mail::Origin::Builtin => 2,
    }
}

/// A template name has to survive a URL path and a `POST /api/mail` field, so it is
/// held to something that reads the same in both: no whitespace, no control
/// characters, bounded length.
fn validate_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty() {
        return Err(invalid("a mail template needs a name"));
    }
    if name.chars().count() > lazypock_db::mail_templates::MAX_NAME_LEN {
        return Err(invalid(format!(
            "a mail template name is limited to {} characters",
            lazypock_db::mail_templates::MAX_NAME_LEN
        )));
    }
    if name
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(invalid(
            "a mail template name must not contain whitespace or control characters",
        ));
    }
    Ok(())
}

/// A name nothing knows.
///
/// 404 rather than 400: the caller named a resource and there is no such resource,
/// which is different from naming one badly. The *send* path answers 400 for the same
/// situation because a template there is a field in a request body, not a path.
fn not_found(name: &str, templates: &lazypock_mail::Templates) -> ApiError {
    ApiError::new(Error::not_found(format!(
        "no mail template named {name:?} (known: {})",
        templates.names().join(", ")
    )))
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(Error::invalid(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_cannot_survive_a_url_is_refused() {
        for name in ["otp", "receipt-2024", "welcome_v2"] {
            validate_name(name).unwrap_or_else(|err| panic!("{name}: {err:?}"));
        }
        for name in [
            "",
            "two words",
            "line\nbreak",
            &"a".repeat(lazypock_db::mail_templates::MAX_NAME_LEN + 1),
        ] {
            assert!(validate_name(name).is_err(), "{name:?} should be refused");
        }
    }

    #[test]
    fn the_strongest_layer_is_the_one_reported() {
        // Newest first: a database body outranks a file, which outranks a built-in.
        assert!(rank(lazypock_mail::Origin::Database) < rank(lazypock_mail::Origin::File));
        assert!(rank(lazypock_mail::Origin::File) < rank(lazypock_mail::Origin::Builtin));
    }
}
