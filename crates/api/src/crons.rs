//! Persisted cron jobs: the admin API over `_crons`.
//!
//! This is the surface Studio's cron page uses — list, create, update, delete, a
//! validation/preview helper, run-now — and the scheduler that fires a job once
//! `next_run_at` arrives ([`run_due`], driven by a ticker in the binary).
//!
//! Superuser-only throughout. Running a job executes an operator-supplied SQL
//! statement or HTTP request, so it can never be reachable without a superuser
//! token.

use std::str::FromStr;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use croner::Cron;
use lazypock_core::error::Error;
use lazypock_core::id::new_id;
use lazypock_db::deadpool_postgres::Client;
use lazypock_db::pool::db_error;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::audit;
use crate::auth::Superuser;
use crate::error::ApiError;
use crate::state::AppState;

/// The full row, with timestamps rendered as RFC 3339, which is what the client
/// parses. Kept in one place so the list and single-row reads cannot drift.
const SELECT: &str = "select id, name, expression, timezone, enabled, action, config, \
     last_status, last_error, last_duration_ms, \
     to_char(last_run_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as last_run_at, \
     to_char(next_run_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as next_run_at, \
     to_char(created at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as created, \
     to_char(updated at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as updated \
     from _crons";

/// Moves a job's schedule forward. Shared by the manual run and the scheduler, so
/// a manual run moves the schedule exactly as an automatic one does.
const ADVANCE_SCHEDULE: &str =
    "update _crons set next_run_at = to_timestamp($2), updated = now() where id = $1";

/// Records what a completed run did. Shared by both paths so they cannot drift in
/// what they write.
const RECORD_RUN: &str = "update _crons set last_run_at = now(), last_status = $2, \
     last_duration_ms = $3, last_error = $4, updated = now() where id = $1";

/// What a create or update sends.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CronInput {
    /// Job name.
    #[serde(default)]
    pub name: Option<String>,
    /// Cron expression, 5-field (or 6-field with seconds).
    #[serde(default)]
    pub expression: Option<String>,
    /// IANA timezone the expression is read in.
    #[serde(default)]
    pub timezone: Option<String>,
    /// Whether the job is live.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// `http`, `sql` or `hook`.
    #[serde(default)]
    pub action: Option<String>,
    /// Action settings.
    #[serde(default)]
    pub config: Option<Value>,
}

/// What `POST /api/crons/validate` sends.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidateBody {
    /// The expression to check.
    #[serde(default)]
    pub expression: Option<String>,
    /// The timezone to read it in.
    #[serde(default)]
    pub timezone: Option<String>,
}

/// The stored fields an update needs to merge against and a run needs to execute.
struct Job {
    /// The row's id, which a run needs in order to record its outcome.
    id: String,
    name: String,
    expression: String,
    timezone: String,
    enabled: bool,
    action: String,
    config: Value,
}

impl Job {
    /// Read the columns every query in this module selects.
    fn from_row(row: &lazypock_db::tokio_postgres::Row) -> Self {
        Self {
            id: row.get("id"),
            name: row.get("name"),
            expression: row.get("expression"),
            timezone: row.get("timezone"),
            enabled: row.get("enabled"),
            action: row.get("action"),
            config: row.get("config"),
        }
    }
}

/// `GET /api/crons`
pub async fn list(State(state): State<AppState>, _: Superuser) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let rows = client
        .query(&format!("{SELECT} order by name"), &[])
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let items: Vec<Value> = rows.iter().map(job_json).collect();
    Ok(Json(json!({ "items": items })))
}

/// `GET /api/crons/{id}`
pub async fn get(
    State(state): State<AppState>,
    _: Superuser,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let row = fetch_row(&client, &id)
        .await?
        .ok_or_else(|| not_found(&id))?;
    Ok(Json(job_json(&row)))
}

/// `POST /api/crons`
pub async fn create(
    State(state): State<AppState>,
    superuser: Superuser,
    Json(input): Json<CronInput>,
) -> Result<Response, ApiError> {
    let prepared = prepare_create(input)?;
    let client = state.db.client().await.map_err(ApiError::new)?;

    if name_taken(&client, &prepared.name, None).await? {
        return Err(ApiError::new(Error::conflict(format!(
            "a cron job named {:?} already exists",
            prepared.name
        ))));
    }

    let next = next_run_at(&prepared.expression, &prepared.timezone)?;
    let id = new_id();
    client
        .execute(
            "insert into _crons \
             (id, name, expression, timezone, enabled, action, config, next_run_at) \
             values ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8))",
            &[
                &id,
                &prepared.name,
                &prepared.expression,
                &prepared.timezone,
                &prepared.enabled,
                &prepared.action,
                &prepared.config,
                &next,
            ],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    tracing::info!(cron = %prepared.name, "cron job created");
    // A cron runs an operator-supplied statement on a timer, so creating one is a
    // way to arrange for work to happen later. It belongs in the trail.
    audit::record(
        &client,
        &superuser.id,
        audit::action::CRON_CHANGE,
        Some(&prepared.name),
        json!({ "change": "create", "action": prepared.action }),
    )
    .await
    .map_err(ApiError::new)?;
    let row = fetch_row(&client, &id)
        .await?
        .ok_or_else(|| ApiError::new(Error::internal("the created cron job vanished")))?;
    Ok((StatusCode::CREATED, Json(job_json(&row))).into_response())
}

/// `PATCH /api/crons/{id}`
pub async fn update(
    State(state): State<AppState>,
    superuser: Superuser,
    Path(id): Path<String>,
    Json(input): Json<CronInput>,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let existing = fetch_job(&client, &id)
        .await?
        .ok_or_else(|| not_found(&id))?;
    let prepared = prepare_update(input, &existing)?;

    if name_taken(&client, &prepared.name, Some(&id)).await? {
        return Err(ApiError::new(Error::conflict(format!(
            "a cron job named {:?} already exists",
            prepared.name
        ))));
    }

    let next = next_run_at(&prepared.expression, &prepared.timezone)?;
    client
        .execute(
            "update _crons set name = $2, expression = $3, timezone = $4, enabled = $5, \
                    action = $6, config = $7, next_run_at = to_timestamp($8), updated = now() \
              where id = $1",
            &[
                &id,
                &prepared.name,
                &prepared.expression,
                &prepared.timezone,
                &prepared.enabled,
                &prepared.action,
                &prepared.config,
                &next,
            ],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    tracing::info!(cron = %prepared.name, "cron job updated");
    audit::record(
        &client,
        &superuser.id,
        audit::action::CRON_CHANGE,
        Some(&prepared.name),
        json!({ "change": "update", "action": prepared.action }),
    )
    .await
    .map_err(ApiError::new)?;
    let row = fetch_row(&client, &id)
        .await?
        .ok_or_else(|| not_found(&id))?;
    Ok(Json(job_json(&row)))
}

/// `DELETE /api/crons/{id}`
pub async fn delete(
    State(state): State<AppState>,
    superuser: Superuser,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let removed = client
        .execute("delete from _crons where id = $1", &[&id])
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    if removed == 0 {
        return Err(not_found(&id));
    }
    tracing::info!(cron = %id, "cron job deleted");
    audit::record(
        &client,
        &superuser.id,
        audit::action::CRON_CHANGE,
        Some(&id),
        json!({ "change": "delete" }),
    )
    .await
    .map_err(ApiError::new)?;
    Ok(Json(json!({})))
}

/// `POST /api/crons/validate`
///
/// Answers `{valid, error}` and, when valid, the next five runs. An invalid
/// expression is a 200 with `valid: false`, because a live preview is asking a
/// question rather than making a failed request.
pub async fn validate(
    _: Superuser,
    Json(body): Json<ValidateBody>,
) -> Result<Json<Value>, ApiError> {
    let expression = body.expression.unwrap_or_default();
    let timezone = body.timezone.unwrap_or_else(|| "UTC".to_string());

    match next_runs(&expression, &timezone, 5) {
        Ok(runs) => Ok(Json(json!({ "valid": true, "nextRuns": runs }))),
        Err(message) => Ok(Json(json!({ "valid": false, "error": message }))),
    }
}

/// `POST /api/crons/{id}` — run now, and record the outcome.
pub async fn run_now(
    State(state): State<AppState>,
    superuser: Superuser,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let job = fetch_job(&client, &id)
        .await?
        .ok_or_else(|| not_found(&id))?;

    let started = Instant::now();
    let (status, error) = execute(&state, &job).await?;
    let duration_ms = started.elapsed().as_millis().min(i32::MAX as u128) as i32;
    // Recompute from now, so a manual run moves the schedule forward exactly as an
    // automatic one will.
    let next = next_run_at(&job.expression, &job.timezone).ok();

    client
        .execute(ADVANCE_SCHEDULE, &[&id, &next])
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;
    client
        .execute(RECORD_RUN, &[&id, &status, &duration_ms, &error])
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    tracing::info!(cron = %job.name, status = %status, "cron job run");
    audit::record(
        &client,
        &superuser.id,
        audit::action::CRON_RUN,
        Some(&job.name),
        json!({ "status": status }),
    )
    .await
    .map_err(ApiError::new)?;
    let row = fetch_row(&client, &id)
        .await?
        .ok_or_else(|| not_found(&id))?;
    Ok(Json(job_json(&row)))
}

/// Fire every cron whose `next_run_at` has arrived, returning how many ran.
///
/// Claiming and advancing the schedule happen in one transaction *before* any
/// action runs, which is what stops two nodes firing the same job: the row is
/// locked with `for update skip locked` while the claim is decided, and the claim
/// is committed before the action starts. The trade is that a crash between that
/// commit and the action loses one run — the alternative is a job that fires twice,
/// which is worse for anything with a side effect.
///
/// A job that cannot run is recorded as an error rather than skipped, so the
/// schedule never advances with nothing to show for it.
pub async fn run_due(state: &AppState, limit: i64) -> Result<usize, ApiError> {
    let due = claim_due(state, limit).await?;
    let ran = due.len();

    for job in due {
        let started = Instant::now();
        let (status, error) = match execute(state, &job).await {
            Ok(outcome) => outcome,
            Err(err) => ("error".to_string(), Some(err.error.message().to_string())),
        };
        let duration_ms = started.elapsed().as_millis().min(i32::MAX as u128) as i32;

        let client = state.db.client().await.map_err(ApiError::new)?;
        client
            .execute(RECORD_RUN, &[&job.id, &status, &duration_ms, &error])
            .await
            .map_err(|err| ApiError::new(db_error(err)))?;

        tracing::info!(cron = %job.name, status = %status, "cron job fired on schedule");
    }

    Ok(ran)
}

/// Take the jobs that are due and move their schedules forward.
async fn claim_due(state: &AppState, limit: i64) -> Result<Vec<Job>, ApiError> {
    let mut client = state.db.client().await.map_err(ApiError::new)?;
    let transaction = client
        .transaction()
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let rows = transaction
        .query(
            "select id, name, expression, timezone, enabled, action, config from _crons \
              where enabled and next_run_at <= now() \
              order by next_run_at \
              limit $1 \
              for update skip locked",
            &[&limit],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let mut claimed = Vec::with_capacity(rows.len());
    for row in &rows {
        let job = Job::from_row(row);
        let next = next_run_at(&job.expression, &job.timezone).ok();
        transaction
            .execute(ADVANCE_SCHEDULE, &[&job.id, &next])
            .await
            .map_err(|err| ApiError::new(db_error(err)))?;
        claimed.push(job);
    }

    transaction
        .commit()
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(claimed)
}

/// Execute a job's action, returning its status and optional error message.
async fn execute(state: &AppState, job: &Job) -> Result<(String, Option<String>), ApiError> {
    match job.action.as_str() {
        "sql" => {
            let statement = job
                .config
                .get("statement")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let client = state.db.client().await.map_err(ApiError::new)?;
            match client.batch_execute(statement).await {
                Ok(()) => Ok(("ok".to_string(), None)),
                // `db_error` rather than the driver's own message: that is only
                // "db error", which leaves `last_error` with nothing to act on.
                Err(err) => Ok(("error".to_string(), Some(db_error(err).to_string()))),
            }
        }
        "http" => {
            let url = job
                .config
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let method = job
                .config
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET");
            let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| {
                ApiError::new(Error::invalid(format!("unknown HTTP method {method:?}")))
            })?;

            let mut request = reqwest::Client::new().request(method, url);
            if let Some(headers) = job.config.get("headers").and_then(Value::as_object) {
                for (name, value) in headers {
                    if let Some(value) = value.as_str() {
                        request = request.header(name, value);
                    }
                }
            }
            if let Some(body) = job.config.get("body").and_then(Value::as_str) {
                request = request.body(body.to_string());
            }

            match request.send().await {
                Ok(response) if response.status().is_success() => Ok(("ok".to_string(), None)),
                Ok(response) => Ok((
                    "error".to_string(),
                    Some(format!("the request returned HTTP {}", response.status())),
                )),
                Err(err) => Ok(("error".to_string(), Some(err.to_string()))),
            }
        }
        // A hook cron needs the hook runtime's `$db`/`$http` bindings, which PLAN.md
        // section 17 records as deliberately undone. Refused loudly rather than
        // reported as a success that did nothing.
        "hook" => Err(ApiError::new(Error::unsupported(
            "running a hook cron is not implemented yet (PLAN.md Track I)",
        ))),
        other => Err(ApiError::new(Error::invalid(format!(
            "unknown cron action {other:?}"
        )))),
    }
}

/// The values a create or update will store.
struct Prepared {
    name: String,
    expression: String,
    timezone: String,
    enabled: bool,
    action: String,
    config: Value,
}

/// Validate a create payload, filling in the defaults.
fn prepare_create(input: CronInput) -> Result<Prepared, ApiError> {
    let name = input.name.unwrap_or_default().trim().to_string();
    if name.is_empty() {
        return Err(invalid("a cron job needs a name"));
    }
    let expression = input.expression.unwrap_or_default().trim().to_string();
    if expression.is_empty() {
        return Err(invalid("a cron job needs an expression"));
    }
    let prepared = Prepared {
        name,
        expression,
        timezone: input.timezone.unwrap_or_else(|| "UTC".to_string()),
        enabled: input.enabled.unwrap_or(true),
        action: input.action.unwrap_or_else(|| "http".to_string()),
        config: input.config.unwrap_or_else(|| json!({})),
    };
    validate_prepared(&prepared)?;
    Ok(prepared)
}

/// Merge an update payload over the stored job, then validate the result.
fn prepare_update(input: CronInput, existing: &Job) -> Result<Prepared, ApiError> {
    let prepared = Prepared {
        name: input.name.unwrap_or_else(|| existing.name.clone()),
        expression: input
            .expression
            .unwrap_or_else(|| existing.expression.clone()),
        timezone: input.timezone.unwrap_or_else(|| existing.timezone.clone()),
        enabled: input.enabled.unwrap_or(existing.enabled),
        action: input.action.unwrap_or_else(|| existing.action.clone()),
        config: input.config.unwrap_or_else(|| existing.config.clone()),
    };
    validate_prepared(&prepared)?;
    Ok(prepared)
}

/// Refuse a job that does not parse, names an unknown timezone, or whose action
/// settings cannot drive the action. Storing a job that can never run, or silently
/// cannot, is the failure this prevents.
fn validate_prepared(prepared: &Prepared) -> Result<(), ApiError> {
    if prepared.name.trim().is_empty() {
        return Err(invalid("a cron job needs a name"));
    }
    parse_expression(&prepared.expression).map_err(invalid)?;
    parse_timezone(&prepared.timezone).map_err(invalid)?;
    validate_action(&prepared.action, &prepared.config).map_err(invalid)?;
    Ok(())
}

/// Check the action name and that its config has what the action needs.
///
/// The action names are the same closed set as the `_crons.action` check
/// constraint, so a job the code accepts the database also accepts.
fn validate_action(action: &str, config: &Value) -> Result<(), String> {
    let text = |key: &str| config.get(key).and_then(Value::as_str).unwrap_or("");
    match action {
        "http" if text("url").trim().is_empty() => {
            Err("an http cron needs a `url` in its config".to_string())
        }
        "sql" if text("statement").trim().is_empty() => {
            Err("a sql cron needs a `statement` in its config".to_string())
        }
        "hook" if text("event").trim().is_empty() => {
            Err("a hook cron needs an `event` in its config".to_string())
        }
        "http" | "sql" | "hook" => Ok(()),
        other => Err(format!(
            "unknown cron action {other:?}; expected one of http, sql, hook"
        )),
    }
}

/// Parse a cron expression, or explain why not.
fn parse_expression(expression: &str) -> Result<Cron, String> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err("Expression is required".to_string());
    }
    Cron::from_str(expression).map_err(|err| err.to_string())
}

/// Resolve an IANA timezone name.
fn parse_timezone(timezone: &str) -> Result<Tz, String> {
    timezone
        .trim()
        .parse::<Tz>()
        .map_err(|_| format!("Unknown timezone {timezone:?}"))
}

/// The next `count` runs of an expression in a timezone, as RFC 3339.
fn next_runs(expression: &str, timezone: &str, count: usize) -> Result<Vec<String>, String> {
    let cron = parse_expression(expression)?;
    let timezone = parse_timezone(timezone)?;
    let mut cursor = timezone.from_utc_datetime(&Utc::now().naive_utc());
    let mut runs = Vec::with_capacity(count);
    for _ in 0..count {
        cursor = cron
            .find_next_occurrence(&cursor, false)
            .map_err(|err| err.to_string())?;
        runs.push(cursor.to_rfc3339());
    }
    Ok(runs)
}

/// The next run as a Unix timestamp, which `to_timestamp` accepts without any
/// date-type mapping between the two libraries.
fn next_run_at(expression: &str, timezone: &str) -> Result<f64, ApiError> {
    let cron = parse_expression(expression).map_err(invalid)?;
    let timezone = parse_timezone(timezone).map_err(invalid)?;
    let now = timezone.from_utc_datetime(&Utc::now().naive_utc());
    let next = cron
        .find_next_occurrence(&now, false)
        .map_err(|err| invalid(format!("cannot schedule the expression: {err}")))?;
    Ok(next.timestamp() as f64)
}

/// A row with the stored fields.
async fn fetch_job(client: &Client, id: &str) -> Result<Option<Job>, ApiError> {
    let row = client
        .query_opt(
            "select id, name, expression, timezone, enabled, action, config from _crons where id = $1",
            &[&id],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(row.map(|row| Job::from_row(&row)))
}

/// A row with every column the client reads.
async fn fetch_row(
    client: &Client,
    id: &str,
) -> Result<Option<lazypock_db::tokio_postgres::Row>, ApiError> {
    client
        .query_opt(&format!("{SELECT} where id = $1"), &[&id])
        .await
        .map_err(|err| ApiError::new(db_error(err)))
}

/// Whether a different job already holds this name.
async fn name_taken(client: &Client, name: &str, except: Option<&str>) -> Result<bool, ApiError> {
    let row = client
        .query_opt(
            "select 1 from _crons where name = $1 and ($2::text is null or id <> $2)",
            &[&name, &except],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;
    Ok(row.is_some())
}

/// One job as the client reads it.
fn job_json(row: &lazypock_db::tokio_postgres::Row) -> Value {
    json!({
        "id": row.get::<_, String>("id"),
        "name": row.get::<_, String>("name"),
        "expression": row.get::<_, String>("expression"),
        "timezone": row.get::<_, String>("timezone"),
        "enabled": row.get::<_, bool>("enabled"),
        "action": row.get::<_, String>("action"),
        "config": row.get::<_, Value>("config"),
        "last_status": row.get::<_, Option<String>>("last_status"),
        "last_error": row.get::<_, Option<String>>("last_error"),
        "last_duration_ms": row.get::<_, Option<i32>>("last_duration_ms"),
        "last_run_at": row.get::<_, Option<String>>("last_run_at"),
        "next_run_at": row.get::<_, Option<String>>("next_run_at"),
        "created": row.get::<_, Option<String>>("created"),
        "updated": row.get::<_, Option<String>>("updated"),
    })
}

/// A 404 for a job that is not there.
fn not_found(id: &str) -> ApiError {
    ApiError::new(Error::not_found(format!("no cron job with id {id:?}")))
}

/// A 400 with the given message.
fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(Error::invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_expression_yields_five_future_runs() {
        let runs = next_runs("0 0 * * *", "UTC", 5).expect("valid");
        assert_eq!(runs.len(), 5);
        for run in &runs {
            assert!(chrono::DateTime::parse_from_rfc3339(run).is_ok(), "{run}");
        }
    }

    #[test]
    fn an_unknown_timezone_is_refused_by_name() {
        let error = next_runs("0 0 * * *", "Mars/Olympus", 1).expect_err("unknown");
        assert!(error.contains("Mars/Olympus"), "{error}");
    }

    #[test]
    fn a_malformed_expression_is_refused() {
        assert!(next_runs("not a cron", "UTC", 1).is_err());
        assert!(next_runs("", "UTC", 1).is_err());
    }

    #[test]
    fn a_timezone_changes_the_wall_clock_result() {
        // Midnight in New York is not midnight UTC, so the runs differ.
        let utc = next_runs("0 0 * * *", "UTC", 1).expect("utc");
        let new_york = next_runs("0 0 * * *", "America/New_York", 1).expect("ny");
        assert_ne!(utc, new_york);
    }

    #[test]
    fn the_action_set_matches_the_check_constraint() {
        // The three names the `_crons.action` constraint allows, plus a config that
        // satisfies each.
        assert!(validate_action("http", &json!({ "url": "https://example.com" })).is_ok());
        assert!(validate_action("sql", &json!({ "statement": "select 1" })).is_ok());
        assert!(validate_action("hook", &json!({ "event": "record.create" })).is_ok());

        // Anything else is refused, and an action missing its settings too, rather
        // than stored as a job that can never run.
        assert!(validate_action("shell", &json!({})).is_err());
        assert!(validate_action("http", &json!({})).is_err());
        assert!(validate_action("sql", &json!({})).is_err());
        assert!(validate_action("hook", &json!({})).is_err());
    }
}
