//! `POST /api/mail` — application email through the configured SMTP.
//!
//! SMTP configured in Studio → Settings → Mail used to power only the server's own
//! auth emails (verification, password reset, OTP, email change). Application mail —
//! a receipt, an invite, a notification — had no way in: there was no endpoint, and a
//! TypeScript hook cannot reach the mailer. This is that way in.
//!
//! It **queues** rather than sends: an SMTP provider that is slow or down must not
//! hold a request open, and the job queue already retries with backoff. The response
//! therefore means "queued", not "delivered"; delivery failures surface on the job
//! (`GET /api/logs` records the send, and a job that exhausts its attempts is marked
//! `dead` with the reason).
//!
//! Two credentials are accepted: a superuser token (the Studio's test button) and an
//! **API key** (an application server), because an app should not need a superuser's
//! password to send mail. The endpoint is not public: an unauthenticated caller
//! cannot use the deployment as a relay.

use axum::extract::{Extension, State};
use axum::Json;
use lazypock_core::error::Error;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::audit;
use crate::auth::{require_superuser_or_api_key, Principal};
use crate::error::ApiError;
use crate::jobs;
use crate::state::AppState;

/// The largest HTML or text body one message may carry.
///
/// The payload is stored in `_jobs`, so it is bounded; 256 KiB is far more than an
/// email body needs and small enough that a mistake cannot fill the table.
const MAX_BODY_BYTES: usize = 256 * 1024;
/// The largest subject, in characters (RFC 5322 allows more; nobody needs it).
const MAX_SUBJECT_CHARS: usize = 300;
/// The largest `assigns` object, serialized.
const MAX_ASSIGNS_BYTES: usize = 64 * 1024;
/// The longest address accepted (the SMTP path limit).
const MAX_ADDRESS_LEN: usize = 254;

/// The body of `POST /api/mail`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendMail {
    /// The recipient's address.
    pub to: String,
    /// The recipient's display name, used by templates as `name`.
    #[serde(default)]
    pub to_name: String,
    /// A template name — a built-in auth template or one the deployment added.
    #[serde(default)]
    pub template: Option<String>,
    /// The template context.
    #[serde(default)]
    pub assigns: Option<Value>,
    /// The subject of a raw message.
    #[serde(default)]
    pub subject: Option<String>,
    /// The HTML body of a raw message.
    #[serde(default)]
    pub html: Option<String>,
    /// The plain-text body of a raw message. Derived from the HTML when omitted.
    #[serde(default)]
    pub text: Option<String>,
}

/// `POST /api/mail`
///
/// Queues one message and answers with the job id. Exactly one of `template` or
/// `subject` must be given; with `subject`, a body (`html` and/or `text`) is required.
pub async fn send(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<SendMail>,
) -> Result<Json<Value>, ApiError> {
    require_superuser_or_api_key(&principal)?;

    let to = body.to.trim();
    validate_address(to)?;
    let to_name = body.to_name.trim().to_string();

    // Acquired before the message shape is settled: the names a send may use include
    // whatever a superuser has saved, which only the database knows.
    let client = state.db.client().await.map_err(ApiError::new)?;

    let (job, template) = match (body.template.as_deref(), body.subject.as_deref()) {
        (Some(template), None) => {
            if body.html.is_some() || body.text.is_some() {
                return Err(invalid(
                    "send either a `template` or a `subject` with a body, not both",
                ));
            }
            let template = template.trim();
            let templates =
                crate::mail_templates::effective(&client, &state.mail_templates).await?;
            let known = templates.names();
            if !known.contains(&template) {
                return Err(invalid(format!(
                    "no mail template named {template:?} (known: {})",
                    known.join(", ")
                )));
            }
            let assigns = body.assigns.unwrap_or(Value::Null);
            let size = serde_json::to_vec(&assigns)
                .map(|encoded| encoded.len())
                .unwrap_or(usize::MAX);
            if size > MAX_ASSIGNS_BYTES {
                return Err(invalid(format!(
                    "`assigns` is {size} bytes; the maximum is {MAX_ASSIGNS_BYTES}"
                )));
            }
            (
                jobs::mail_job(template, &to_name, to, assigns),
                Some(template.to_string()),
            )
        }
        (None, Some(subject)) => {
            let subject = subject.trim();
            if subject.is_empty() {
                return Err(invalid("`subject` must not be empty"));
            }
            if subject.chars().count() > MAX_SUBJECT_CHARS {
                return Err(invalid(format!(
                    "`subject` is longer than {MAX_SUBJECT_CHARS} characters"
                )));
            }
            // A newline in a header is how a header injection starts; the mailer
            // would encode it, but there is no reason to accept it.
            if subject.chars().any(|character| character.is_control()) {
                return Err(invalid("`subject` must not contain control characters"));
            }

            let html = body.html.unwrap_or_default();
            let text = body.text.unwrap_or_default();
            if html.trim().is_empty() && text.trim().is_empty() {
                return Err(invalid("a message needs `html` or `text`"));
            }
            if html.len() > MAX_BODY_BYTES || text.len() > MAX_BODY_BYTES {
                return Err(invalid(format!(
                    "a mail body is limited to {MAX_BODY_BYTES} bytes"
                )));
            }
            (
                jobs::raw_mail_job(&to_name, to, subject, &html, &text),
                None,
            )
        }
        (Some(_), Some(_)) => {
            return Err(invalid("send either a `template` or a `subject`, not both"))
        }
        (None, None) => {
            return Err(invalid(
                "a message needs either a `template` or a `subject`",
            ))
        }
    };

    // Refuse before queueing when SMTP is not configured: a job that can only fail
    // would retry and be marked dead, and the caller would learn nothing.
    let settings = jobs::read_settings(&client)
        .await
        .map_err(|err| ApiError::new(Error::internal(err)))?;
    lazypock_mail::MailConfig::from_settings(&settings)
        .map_err(ApiError::new)?
        .ok_or_else(|| {
            ApiError::new(Error::invalid(
                "mail is not configured: set it in Studio → Settings → Mail",
            ))
        })?;

    let job_id = jobs::enqueue(&client, job).await?;

    audit::record(
        &client,
        &audit::actor(&principal),
        audit::action::MAIL_SEND,
        Some(to),
        json!({ "template": template, "job": job_id }),
    )
    .await
    .map_err(ApiError::new)?;

    Ok(Json(json!({ "queued": true, "id": job_id })))
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(Error::invalid(message))
}

/// A shape check on the recipient, not a delivery check.
///
/// The SMTP server is the authority on whether an address exists; this only refuses
/// what cannot be an address (no `@`, whitespace, control characters, absurd length),
/// so a typo fails here rather than as a queue entry that retries three times.
fn validate_address(address: &str) -> Result<(), ApiError> {
    if address.is_empty() {
        return Err(invalid("`to` is required"));
    }
    if address.len() > MAX_ADDRESS_LEN {
        return Err(invalid(format!(
            "`to` is longer than {MAX_ADDRESS_LEN} characters"
        )));
    }
    if address
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(invalid(
            "`to` must not contain whitespace or control characters",
        ));
    }
    // Exactly one `@`: `rsplit_once` would otherwise put a second one in the local
    // part (`a@b@example.com`), which is not an address this server should queue.
    if address.matches('@').count() != 1 {
        return Err(invalid("`to` must be an email address"));
    }
    let Some((local, domain)) = address.rsplit_once('@') else {
        return Err(invalid("`to` must be an email address"));
    };
    if local.is_empty() || domain.is_empty() {
        return Err(invalid("`to` must be an email address"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plausible_addresses_are_accepted() {
        for address in ["a@example.com", "first.last+tag@mail.example.co.uk"] {
            validate_address(address).unwrap_or_else(|err| panic!("{address}: {err:?}"));
        }
    }

    #[test]
    fn what_cannot_be_an_address_is_refused() {
        for address in [
            "",
            "no-at-sign",
            "@example.com",
            "a@",
            "a@b@example.com",
            "a b@example.com",
            "a@example.com\r\nBcc: victim@example.com",
            &format!("{}@example.com", "a".repeat(MAX_ADDRESS_LEN)),
        ] {
            assert!(
                validate_address(address).is_err(),
                "{address:?} should have been refused"
            );
        }
    }
}
