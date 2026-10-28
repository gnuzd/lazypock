//! The background job queue over `_jobs`.
//!
//! `_jobs` has existed since migration 1, with `_jobs_ready_idx` to claim from,
//! and nothing wrote to it — the table and its index were the whole feature. This
//! is the worker and the enqueue path.
//!
//! The claim is the same shape as the cron scheduler's, for the same reason: a row
//! is locked with `for update skip locked` and moved to `running` inside one
//! transaction, so two nodes cannot take the same job and a slow job does not hold
//! up the ones behind it.
//!
//! A job that fails is retried with a growing delay and, once its attempt budget
//! is spent, retired as `dead` with the reason kept in `last_error`. A worker that
//! dies mid-job leaves a `running` row, which [`run_due`] reclaims once the lease
//! has passed — and that counts as an attempt, so a job that repeatedly kills its
//! worker cannot loop forever.
//!
//! Two kinds today: `sql`, which exercises the machinery end to end, and `mail`,
//! which sends a templated email. The "after" hooks are the next caller.

use std::sync::Arc;
use std::time::Instant;

use lazypock_core::error::Error;
use lazypock_core::id::new_id;
use lazypock_db::deadpool_postgres::Client;
use lazypock_db::pool::db_error;
use serde_json::Value;

use crate::error::ApiError;
use crate::state::AppState;

/// A job that runs a SQL statement.
pub const KIND_SQL: &str = "sql";

/// A job that sends one templated email.
pub const KIND_MAIL: &str = "mail";

/// How many attempts a job gets unless it asks for something else.
pub const DEFAULT_MAX_ATTEMPTS: i32 = 5;

/// The longest a retry will be delayed.
const MAX_BACKOFF_SECONDS: f64 = 300.0;

/// A job to enqueue.
#[derive(Debug, Clone)]
pub struct NewJob {
    /// The dispatch key: which handler runs it.
    pub kind: String,
    /// The job's data, handed to the handler.
    pub payload: Value,
    /// Among jobs that are equally due, higher runs first.
    pub priority: i32,
    /// How many attempts before the job is retired as `dead`.
    pub max_attempts: i32,
    /// How long to wait before the first attempt.
    pub delay: std::time::Duration,
}

impl NewJob {
    /// A job of this kind, due immediately, with the default attempt budget.
    pub fn new(kind: impl Into<String>, payload: Value) -> Self {
        Self {
            kind: kind.into(),
            payload,
            priority: 0,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            delay: std::time::Duration::ZERO,
        }
    }

    /// Wait this long before the first attempt.
    pub fn after(mut self, delay: std::time::Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Retire the job after this many attempts instead of the default.
    pub fn with_max_attempts(mut self, max_attempts: i32) -> Self {
        self.max_attempts = max_attempts;
        self
    }
}

/// Add a job to the queue, returning its id.
///
/// Callers inside a transaction use the same connection, so an enqueue can be
/// committed with the change that caused it — a record written and the job that
/// reacts to it either both happen or neither does.
pub async fn enqueue(client: &Client, job: NewJob) -> Result<String, ApiError> {
    let id = new_id();
    let delay = job.delay.as_secs_f64();

    client
        .execute(
            "insert into _jobs (id, kind, payload, priority, max_attempts, run_at) \
             values ($1, $2, $3, $4, $5, now() + make_interval(secs => $6))",
            &[
                &id,
                &job.kind,
                &job.payload,
                &job.priority,
                &job.max_attempts,
                &delay,
            ],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(id)
}

/// Run the jobs that are due, returning how many were attempted.
///
/// `worker` identifies this process in `locked_by`, which is what makes a stale
/// claim visible to the next pass. `lease_seconds` is how long a claim is trusted
/// before the job is handed back.
pub async fn run_due(
    state: &AppState,
    worker: &str,
    limit: i64,
    lease_seconds: f64,
) -> Result<usize, ApiError> {
    reclaim_stale(state, lease_seconds).await?;
    let claimed = claim(state, worker, limit).await?;
    let ran = claimed.len();

    for job in &claimed {
        run_one(state, job).await?;
    }

    Ok(ran)
}

/// A job this worker holds.
struct Claimed {
    id: String,
    kind: String,
    payload: Value,
    attempts: i32,
    max_attempts: i32,
}

/// Take the jobs that are due, marking them held by this worker.
async fn claim(state: &AppState, worker: &str, limit: i64) -> Result<Vec<Claimed>, ApiError> {
    let mut client = state.db.client().await.map_err(ApiError::new)?;
    let transaction = client
        .transaction()
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let rows = transaction
        .query(
            "update _jobs \
                set status = 'running', locked_by = $2, locked_at = now(), updated = now() \
              where id in ( \
                    select id from _jobs \
                     where status = 'pending' and run_at <= now() \
                     order by priority desc, run_at \
                     limit $1 \
                     for update skip locked \
              ) \
             returning id, kind, payload, attempts, max_attempts",
            &[&limit, &worker],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    transaction
        .commit()
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(rows
        .iter()
        .map(|row| Claimed {
            id: row.get("id"),
            kind: row.get("kind"),
            payload: row.get("payload"),
            attempts: row.get("attempts"),
            max_attempts: row.get("max_attempts"),
        })
        .collect())
}

/// Hand back jobs whose worker stopped holding them.
///
/// The attempt is counted: a job that keeps killing its worker has to run out of
/// attempts and become `dead` rather than being retried forever.
async fn reclaim_stale(state: &AppState, lease_seconds: f64) -> Result<u64, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let reclaimed = client
        .execute(
            "update _jobs \
                set attempts = attempts + 1, \
                    status = case when attempts + 1 >= max_attempts then 'dead' else 'pending' end, \
                    run_at = now(), \
                    locked_by = null, \
                    locked_at = null, \
                    last_error = coalesce(last_error, 'the worker holding this job stopped'), \
                    updated = now() \
              where status = 'running' \
                and locked_at < now() - make_interval(secs => $1)",
            &[&lease_seconds],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    if reclaimed > 0 {
        tracing::warn!(reclaimed, "jobs whose worker stopped were handed back");
    }
    Ok(reclaimed)
}

/// Run one claimed job and record what happened.
async fn run_one(state: &AppState, job: &Claimed) -> Result<(), ApiError> {
    let started = Instant::now();
    let outcome = perform(state, job).await;
    let duration_ms = started.elapsed().as_millis().min(i32::MAX as u128) as i32;
    let client = state.db.client().await.map_err(ApiError::new)?;

    match outcome {
        Ok(()) => {
            client
                .execute(
                    "update _jobs set status = 'done', attempts = attempts + 1, locked_by = null, \
                            locked_at = null, last_error = null, updated = now() \
                      where id = $1",
                    &[&job.id],
                )
                .await
                .map_err(|err| ApiError::new(db_error(err)))?;
            tracing::info!(kind = %job.kind, duration_ms, "job done");
        }
        Err(message) => {
            let attempts = job.attempts + 1;
            if attempts >= job.max_attempts {
                client
                    .execute(
                        "update _jobs set status = 'dead', attempts = $2, run_at = now(), \
                                locked_by = null, locked_at = null, last_error = $3, \
                                updated = now() \
                          where id = $1",
                        &[&job.id, &attempts, &message],
                    )
                    .await
                    .map_err(|err| ApiError::new(db_error(err)))?;
                tracing::warn!(
                    kind = %job.kind,
                    attempts,
                    error = %message,
                    "job retired as dead after its last attempt"
                );
            } else {
                let backoff = backoff_seconds(attempts);
                client
                    .execute(
                        "update _jobs set status = 'pending', attempts = $2, \
                                run_at = now() + make_interval(secs => $3), locked_by = null, \
                                locked_at = null, last_error = $4, updated = now() \
                          where id = $1",
                        &[&job.id, &attempts, &backoff, &message],
                    )
                    .await
                    .map_err(|err| ApiError::new(db_error(err)))?;
                tracing::warn!(
                    kind = %job.kind,
                    attempts,
                    retry_in_seconds = backoff,
                    error = %message,
                    "job failed and will be retried"
                );
            }
        }
    }

    Ok(())
}

/// The payload a `mail` job carries.
///
/// Two shapes: a **named template** (the built-in auth emails, or a template file a
/// deployment added) with its context, or a **raw message** (`subject` + `html`/
/// `text`) for application mail sent through `POST /api/mail`.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MailJob {
    /// The template name, when the message comes from a template.
    #[serde(default)]
    template: Option<String>,
    /// The recipient's address.
    to: String,
    /// The recipient's display name.
    to_name: String,
    /// The template context.
    #[serde(default)]
    assigns: Value,
    /// The subject, for a raw message.
    #[serde(default)]
    subject: Option<String>,
    /// The HTML body, for a raw message.
    #[serde(default)]
    html: Option<String>,
    /// The plain-text body, for a raw message.
    #[serde(default)]
    text: Option<String>,
}

/// A job that sends one templated email.
///
/// Callers enqueue this rather than sending in the request: an SMTP server that is
/// slow or down must not hold a request open, and the queue already retries with
/// backoff.
pub fn mail_job(template: &str, to_name: &str, to: &str, assigns: Value) -> NewJob {
    NewJob::new(
        KIND_MAIL,
        serde_json::json!({
            "template": template,
            "to": to,
            "toName": to_name,
            "assigns": assigns,
        }),
    )
}

/// A job that sends one message rendered by the caller (application mail).
pub fn raw_mail_job(to_name: &str, to: &str, subject: &str, html: &str, text: &str) -> NewJob {
    NewJob::new(
        KIND_MAIL,
        serde_json::json!({
            "to": to,
            "toName": to_name,
            "subject": subject,
            "html": html,
            "text": text,
        }),
    )
}

/// The application settings, for a job that needs them (the mail config).
pub(crate) async fn read_settings(client: &Client) -> Result<Value, String> {
    let rows = client
        .query("select key, value from _settings", &[])
        .await
        .map_err(|err| db_error(err).to_string())?;

    let mut settings = serde_json::Map::new();
    for row in &rows {
        let key: String = row.get("key");
        // Lazypock's own settings, including the signing key, are not application
        // settings and one of them is a credential.
        if key.starts_with("auth.") {
            continue;
        }
        settings.insert(key, row.get("value"));
    }
    Ok(Value::Object(settings))
}

/// What the job does, or why it could not.
async fn perform(state: &AppState, job: &Claimed) -> Result<(), String> {
    match job.kind.as_str() {
        KIND_SQL => {
            let statement = job
                .payload
                .get("statement")
                .and_then(Value::as_str)
                .unwrap_or("");
            if statement.trim().is_empty() {
                return Err("a sql job needs a `statement` in its payload".to_string());
            }
            let client = state.db.client().await.map_err(|err| err.to_string())?;
            client
                .batch_execute(statement)
                .await
                // `tokio_postgres::Error`'s own `Display` is only "db error",
                // which leaves `last_error` with nothing to act on. `db_error`
                // pulls the code and message out of the server's response.
                .map_err(|err| db_error(err).to_string())
        }
        KIND_MAIL => {
            let mail: MailJob = serde_json::from_value(job.payload.clone())
                .map_err(|err| format!("invalid mail payload: {err}"))?;

            let client = state.db.client().await.map_err(|err| err.to_string())?;
            let settings = read_settings(&client).await?;
            // Read at send time, so a superuser changing SMTP settings in the Studio
            // takes effect on the next send without a restart.
            let config = lazypock_mail::MailConfig::from_settings(&settings)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| {
                    "mail is not configured: set the `mail` settings before queueing mail"
                        .to_string()
                })?;

            // Resolved at send time, with whatever a superuser has saved applied. A job
            // queued before an edit should still send the edited body: the alternative
            // is a typo in a password-reset email that keeps going out until the queue
            // drains.
            let stored = lazypock_db::mail_templates::load(&client)
                .await
                .map_err(|err| err.to_string())?;
            let overrides: Vec<lazypock_mail::Override> = stored
                .iter()
                .map(|row| lazypock_mail::Override {
                    name: row.name.clone(),
                    part: row.part.clone(),
                    body: row.body.clone(),
                })
                .collect();
            let templates = state
                .mail_templates
                .with_overrides(&overrides)
                .map_err(|err| err.to_string())?;

            let mailer = lazypock_mail::Mailer::new(Arc::new(templates), config)
                .map_err(|err| err.to_string())?;

            if let Some(template) = mail.template.as_deref() {
                return mailer
                    .deliver(template, &mail.to_name, &mail.to, &mail.assigns)
                    .await
                    .map_err(|err| err.to_string());
            }

            let subject = mail.subject.as_deref().unwrap_or_default();
            let html = mail.html.as_deref().unwrap_or_default();
            let text = mail.text.as_deref().unwrap_or_default();
            if subject.trim().is_empty() || (html.trim().is_empty() && text.trim().is_empty()) {
                return Err(
                    "a mail job needs a `template`, or a `subject` and an `html`/`text` body"
                        .to_string(),
                );
            }
            mailer
                .deliver_raw(&mail.to_name, &mail.to, subject, html, text)
                .await
                .map_err(|err| err.to_string())
        }
        // An unknown kind is a bug in whatever enqueued it. Retrying and then
        // retiring it as `dead` with the reason is better than a job that quietly
        // succeeds at doing nothing.
        other => Err(Error::unsupported(format!("unknown job kind {other:?}")).to_string()),
    }
}

/// How long to wait before the next attempt.
///
/// Exponential with a ceiling: a failing job should back off rather than hammer
/// whatever it depends on, and should not disappear for a day either.
fn backoff_seconds(attempts: i32) -> f64 {
    2_f64.powi(attempts.clamp(1, 16)).min(MAX_BACKOFF_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backoff_grows_and_then_holds() {
        assert_eq!(backoff_seconds(1), 2.0);
        assert_eq!(backoff_seconds(2), 4.0);
        assert_eq!(backoff_seconds(3), 8.0);
        // Held at the ceiling rather than growing without bound.
        assert_eq!(backoff_seconds(30), MAX_BACKOFF_SECONDS);
        assert_eq!(backoff_seconds(0), 2.0);
    }
}
