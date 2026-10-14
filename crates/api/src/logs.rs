//! Request logs: the recorder, and the superuser API over `_logs`.
//!
//! Every `/api/*` request except the health probe is timed and recorded. The write
//! happens on a detached task, so an insert can never add latency to the response
//! or fail the request if the database is briefly unavailable — the same choice the
//! implementation this replaced made.
//!
//! Requests are recorded *after* the handler, because the status is part of the
//! entry. A small JSON request body is captured and redacted first; an error
//! response's message is captured too, so the Studio's log detail can show why a
//! request failed. Nothing else is buffered.

use std::time::Instant;

use axum::body::{Body, HttpBody};
use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderName, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lazypock_core::error::Error;
use lazypock_db::deadpool_postgres::Client;
use lazypock_db::pool::{db_error, Db};
use lazypock_db::tokio_postgres::types::ToSql;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::Superuser;
use crate::error::ApiError;
use crate::state::AppState;

/// The `_settings` key the retention lives under. Kept as its own row rather than
/// under a generic key so a settings form cannot overwrite it by accident.
const SETTINGS_KEY: &str = "logs";

/// The largest body buffered for a log entry. Anything larger is recorded without
/// one rather than held in memory.
const MAX_BODY_BYTES: usize = 64 * 1024;

/// Keys whose values are replaced before a request body is stored. Lower-cased for
/// the comparison, so `passwordConfirm` and `password_confirm` both match.
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwordconfirm",
    "password_confirm",
    "oldpassword",
    "newpassword",
    "token",
    "verificationtoken",
    "resettoken",
    "secret",
    "apikey",
    "api_key",
];

/// `GET /api/logs`
#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    /// 1-based page.
    #[serde(default)]
    pub page: Option<i64>,
    /// Items per page.
    #[serde(default, rename = "perPage")]
    pub per_page: Option<i64>,
    /// Only entries for one collection.
    #[serde(default)]
    pub collection: Option<String>,
    /// A status class (`4xx`) or an exact code (`500`).
    #[serde(default)]
    pub status: Option<String>,
    /// `asc` for oldest first; anything else is newest first.
    #[serde(default)]
    pub order: Option<String>,
}

/// `DELETE /api/logs`
#[derive(Debug, Default, Deserialize)]
pub struct DeleteQuery {
    /// `all=true` clears every entry.
    #[serde(default)]
    pub all: Option<String>,
    /// Delete entries older than this many days. `0` deletes nothing.
    #[serde(default)]
    pub days: Option<String>,
}

/// `PATCH /api/logs/retention`
#[derive(Debug, Default, Deserialize)]
pub struct RetentionBody {
    /// `null` or `0` disables auto-clean; a positive number is the retention.
    #[serde(default)]
    pub days: Option<i64>,
}

/// Record every API request after it is handled.
pub async fn record(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    // The health probe is excluded: a liveness check runs continuously and would
    // bury everything else. Non-API paths are the embedded Studio's static assets.
    if !path.starts_with("/api/") || path == "/api/health" {
        return next.run(request).await;
    }

    let ip = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip().to_string())
        .or_else(|| forwarded_for(request.headers()));
    let user_agent = header_string(request.headers(), header::USER_AGENT);
    let referer = header_string(request.headers(), header::REFERER);
    let collection = collection_from_path(&path);

    // The body is taken only when it is small JSON, so a multipart upload or a
    // large payload is never buffered. `to_bytes` cannot give the body back, so
    // the content-length check is what keeps a failed read from emptying it.
    let (parts, body) = request.into_parts();
    let (body, request_body) = if capturable_body(&method, &parts.headers, &body) {
        match axum::body::to_bytes(body, MAX_BODY_BYTES + 1).await {
            Ok(bytes) => {
                let text = redacted_json(&bytes);
                (Body::from(bytes), text)
            }
            Err(_) => {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "the request body is larger than the log recorder buffers",
                )
                    .into_response();
            }
        }
    } else {
        (body, None)
    };
    let request = Request::from_parts(parts, body);

    let response = next.run(request).await;
    let status = response.status().as_u16();
    let duration_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let (response, error) = capture_error(response).await;

    let db = state.db.clone();
    tokio::spawn(async move {
        insert(
            &db,
            Recorded {
                method,
                path,
                status: status as i32,
                duration_ms,
                ip,
                user_agent,
                referer,
                collection,
                error,
                body: request_body,
            },
        )
        .await;
    });

    response
}

/// One recorded entry.
struct Recorded {
    method: String,
    path: String,
    status: i32,
    duration_ms: f64,
    ip: Option<String>,
    user_agent: Option<String>,
    referer: Option<String>,
    collection: Option<String>,
    error: Option<String>,
    body: Option<String>,
}

/// Write one entry. Failures are logged and swallowed: recording a request must
/// never be a reason a request fails.
async fn insert(db: &Db, entry: Recorded) {
    let client = match db.client().await {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!(error = %err, "could not record a request log");
            return;
        }
    };

    let result = client
        .execute(
            "insert into _logs \
             (method, path, status, duration_ms, ip, user_agent, referer, collection, error, body) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            &[
                &entry.method,
                &entry.path,
                &entry.status,
                &entry.duration_ms,
                &entry.ip,
                &entry.user_agent,
                &entry.referer,
                &entry.collection,
                &entry.error,
                &entry.body,
            ],
        )
        .await;

    if let Err(err) = result {
        tracing::warn!(error = %db_error(err), "could not record a request log");
    }
}

/// `GET /api/logs`
pub async fn list(
    State(state): State<AppState>,
    _: Superuser,
    Query(params): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).clamp(1, 200);
    let offset = (page - 1) * per_page;

    let mut conditions = Vec::new();
    let mut args: Vec<Param> = Vec::new();

    if let Some(collection) = params
        .collection
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        args.push(Param::Text(collection.to_string()));
        conditions.push(format!("collection = ${}", args.len()));
    }
    if let Some(filter) = params.status.as_deref().and_then(status_condition) {
        match filter {
            StatusFilter::Exact(code) => {
                args.push(Param::Int(code));
                conditions.push(format!("status = ${}", args.len()));
            }
            StatusFilter::Range(low, high) => {
                args.push(Param::Int(low));
                args.push(Param::Int(high));
                conditions.push(format!(
                    "status >= ${} and status < ${}",
                    args.len() - 1,
                    args.len()
                ));
            }
        }
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" where {}", conditions.join(" and "))
    };

    let count_sql = format!("select count(*) from _logs{where_clause}");
    let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(Param::as_sql).collect();
    let total: i64 = client
        .query_one(&count_sql, &refs)
        .await
        .map_err(|err| ApiError::new(db_error(err)))?
        .get(0);

    let order = if params.order.as_deref() == Some("asc") {
        "asc"
    } else {
        "desc"
    };
    args.push(Param::Big(per_page));
    let limit = format!("${}", args.len());
    args.push(Param::Big(offset));
    let offset_placeholder = format!("${}", args.len());

    // `duration`/`created_at` are the names the client reads; the columns keep the
    // names the table shipped with.
    let list_sql = format!(
        "select id, method, path, status, duration_ms as duration, ip, user_agent, referer, \
                collection, error, body, \
                to_char(created at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as created_at \
           from _logs{where_clause} order by created {order}, id {order} limit {limit} offset {offset_placeholder}"
    );
    let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(Param::as_sql).collect();
    let rows = client
        .query(&list_sql, &refs)
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let items: Vec<Value> = rows.iter().map(log_json).collect();

    Ok(Json(json!({
        "page": page,
        "perPage": per_page,
        "totalItems": total,
        "totalPages": ((total + per_page - 1) / per_page).max(1),
        "items": items,
    })))
}

/// `GET /api/logs/{id}`
pub async fn show(
    State(state): State<AppState>,
    _: Superuser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let row = client
        .query_opt(
            "select id, method, path, status, duration_ms as duration, ip, user_agent, referer, \
                    collection, error, body, \
                    to_char(created at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as created_at \
               from _logs where id = $1",
            &[&id],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?
        .ok_or_else(|| ApiError::new(Error::not_found(format!("no log entry with id {id}"))))?;

    Ok(Json(log_json(&row)))
}

/// `GET /api/logs/stats`
pub async fn stats(
    State(state): State<AppState>,
    _: Superuser,
    Query(query): Query<StatsQuery>,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let (range, interval, bucket) = stats_window(query.range.as_deref());

    let total: i64 = client
        .query_one("select count(*) from _logs", &[])
        .await
        .map_err(|err| ApiError::new(db_error(err)))?
        .get(0);

    // Bucket sizes are fixed strings from `stats_window`, so nothing from the
    // request reaches the SQL. The series is zero-filled for the whole window so
    // the chart is complete even with no data.
    let series_sql = format!(
        "with buckets as ( \
             select date_bin(interval '{bucket}', gs, timestamptz '2000-01-01') as t \
               from generate_series( \
                   date_bin(interval '{bucket}', now(), timestamptz '2000-01-01') - interval '{interval}', \
                   now(), \
                   interval '{bucket}' \
               ) as gs \
         ), \
         agg as ( \
             select date_bin(interval '{bucket}', created, timestamptz '2000-01-01') as t, \
                    count(*) as total, \
                    count(*) filter (where status >= 400) as errors, \
                    avg(duration_ms) as avg_duration \
               from _logs \
              where created > now() - interval '{interval}' \
              group by t \
         ) \
         select to_char(b.t at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as date, \
                coalesce(a.total, 0) as total, \
                coalesce(a.errors, 0) as errors, \
                coalesce(a.avg_duration, 0) as avg_duration \
           from buckets b left join agg a on a.t = b.t order by b.t"
    );
    let rows = client
        .query(&series_sql, &[])
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let series: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "date": row.get::<_, String>("date"),
                "total": row.get::<_, i64>("total"),
                "errors": row.get::<_, i64>("errors"),
                "avg_duration": row.get::<_, f64>("avg_duration"),
            })
        })
        .collect();

    let requests: i64 = series
        .iter()
        .map(|item| item["total"].as_i64().unwrap_or(0))
        .sum();
    let errors: i64 = series
        .iter()
        .map(|item| item["errors"].as_i64().unwrap_or(0))
        .sum();
    let avg_duration: f64 = client
        .query_one(
            &format!(
                "select coalesce(avg(duration_ms), 0) from _logs \
                  where created > now() - interval '{interval}'"
            ),
            &[],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?
        .get(0);

    Ok(Json(json!({
        "total": total,
        "range": range,
        "series": series,
        "summary": {
            "requests": requests,
            "errors": errors,
            "avg_duration": avg_duration,
        },
    })))
}

/// `GET /api/logs/collections`
pub async fn collections(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let rows = client
        .query(
            "select distinct collection from _logs \
              where collection is not null order by collection",
            &[],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let items: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
    Ok(Json(json!({ "items": items })))
}

/// `GET /api/logs/retention`
pub async fn get_retention(
    State(state): State<AppState>,
    _: Superuser,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let days = retention_days(&client).await?;
    Ok(Json(json!({ "days": days })))
}

/// `PATCH /api/logs/retention`
pub async fn set_retention(
    State(state): State<AppState>,
    _: Superuser,
    Json(body): Json<RetentionBody>,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;

    // `0`, `null` and absent all mean "off", which is what the Studio's "off"
    // option sends. A negative value is the same: there is no way to keep logs for
    // minus a week, and refusing it would only add a branch the client cannot hit.
    let days = body.days.filter(|days| *days > 0);

    let value = json!({ "retention_days": days });
    client
        .execute(
            "insert into _settings (key, value, updated) values ($1, $2, now()) \
             on conflict (key) do update set value = excluded.value, updated = now()",
            &[&SETTINGS_KEY.to_string(), &value],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    tracing::info!(days = ?days, "log retention updated");
    Ok(Json(json!({ "days": days })))
}

/// `DELETE /api/logs`
pub async fn delete(
    State(state): State<AppState>,
    _: Superuser,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;

    if query.all.as_deref() == Some("true") {
        client
            .execute("delete from _logs", &[])
            .await
            .map_err(|err| ApiError::new(db_error(err)))?;
        return Ok(Json(json!({ "ok": true, "all": true })));
    }

    // `?days=N` wins; otherwise the configured retention; otherwise the historical
    // default of seven days, so a manual clean still works with auto-clean off.
    // `?days=0` deletes nothing.
    let days = match query.days.as_deref() {
        None => retention_days(&client).await?.or(Some(7)),
        Some(raw) => raw.trim().parse::<i64>().ok().filter(|days| *days > 0),
    };

    let Some(days) = days else {
        return Ok(Json(json!({ "ok": true, "days": null, "deleted": 0 })));
    };

    let deleted = client
        .execute(
            "delete from _logs where created < now() - make_interval(days => $1::int)",
            &[&(days as i32)],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(Json(
        json!({ "ok": true, "days": days, "deleted": deleted }),
    ))
}

/// A bound query parameter.
///
/// Used instead of `Box<dyn ToSql + Sync>` because that trait object is not `Send`,
/// which makes the handler future `!Send` and the route fail to compile. A concrete
/// enum is `Send`, and [`Param::as_sql`] hands out the borrowed trait object the
/// driver wants.
enum Param {
    /// Text.
    Text(String),
    /// A 32-bit integer, for a status code.
    Int(i32),
    /// A 64-bit integer, for a limit or offset.
    Big(i64),
}

impl Param {
    /// This parameter as the driver's parameter type.
    fn as_sql(&self) -> &(dyn ToSql + Sync) {
        match self {
            Param::Text(value) => value,
            Param::Int(value) => value,
            Param::Big(value) => value,
        }
    }
}

/// The configured retention in days, or `None` when auto-clean is off.
async fn retention_days(client: &Client) -> Result<Option<i64>, ApiError> {
    let row = client
        .query_opt(
            "select value from _settings where key = $1",
            &[&SETTINGS_KEY.to_string()],
        )
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(row.and_then(|row| {
        let value: Value = row.get("value");
        value
            .get("retention_days")
            .and_then(Value::as_i64)
            .filter(|days| *days > 0)
    }))
}

/// The range, interval and bucket for a stats request. Only the closed set of
/// ranges is accepted; anything else falls back to 24 hours.
fn stats_window(range: Option<&str>) -> (&'static str, &'static str, &'static str) {
    match range {
        Some("7d") => ("7d", "7 days", "6 hours"),
        Some("30d") => ("30d", "30 days", "1 day"),
        _ => ("24h", "24 hours", "1 hour"),
    }
}

/// A status filter: an exact code, or a whole class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusFilter {
    /// A specific code such as `500`.
    Exact(i32),
    /// A lower-inclusive, upper-exclusive class such as `400..500`.
    Range(i32, i32),
}

/// Parse a status filter. `4xx` and `404` are accepted; anything else is ignored.
fn status_condition(raw: &str) -> Option<StatusFilter> {
    let raw = raw.trim();
    let bytes = raw.as_bytes();
    if bytes.len() != 3 {
        return None;
    }
    if bytes[0].is_ascii_digit() && bytes[1] == b'x' && bytes[2] == b'x' {
        let low = (bytes[0] - b'0') as i32 * 100;
        return Some(StatusFilter::Range(low, low + 100));
    }
    if raw.chars().all(|character| character.is_ascii_digit()) {
        return raw.parse().ok().map(StatusFilter::Exact);
    }
    None
}

/// The collection a records path addresses.
fn collection_from_path(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        ["api", "collections", collection, ..] if !collection.is_empty() => {
            Some((*collection).to_string())
        }
        _ => None,
    }
}

/// Read a header as a string, if it is present and valid.
fn header_string(headers: &HeaderMap, name: HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// The client address from a proxy header, when the connection is not enough. The
/// socket address wins when there is one; this is only a fallback.
fn forwarded_for(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Whether the request body is worth buffering: a small JSON write.
///
/// The bound comes from the body's own size, not the `Content-Length` header: an
/// in-process request (which the tests use) has no such header, and a chunked body
/// has no known size at all. A body whose size is not known is left alone rather
/// than buffered without a bound, so a large import or batch payload is never read
/// into memory just to be logged.
fn capturable_body(method: &str, headers: &HeaderMap, body: &Body) -> bool {
    if !matches!(method, "POST" | "PATCH" | "PUT") {
        return false;
    }
    let json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    let small = body
        .size_hint()
        .exact()
        .is_some_and(|length| length <= MAX_BODY_BYTES as u64);
    json && small
}

/// Parse a JSON body and redact the values of credential-looking keys.
fn redacted_json(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    serde_json::to_string(&redact(value)).ok()
}

/// Replace the values of sensitive keys, recursively.
fn redact(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    if SENSITIVE_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
                        (key, Value::String("[REDACTED]".to_string()))
                    } else {
                        (key, redact(value))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(redact).collect()),
        other => other,
    }
}

/// Read an error response's message so the log can show why it failed, giving the
/// body back unchanged. Only error responses are read, and they are small.
async fn capture_error(response: Response) -> (Response, Option<String>) {
    if !response.status().is_client_error() && !response.status().is_server_error() {
        return (response, None);
    }

    let (parts, body) = response.into_parts();
    match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
        Ok(bytes) => {
            let message = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|value| {
                    value
                        .get("message")
                        .or_else(|| value.get("error"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            (Response::from_parts(parts, Body::from(bytes)), message)
        }
        Err(_) => (Response::from_parts(parts, Body::empty()), None),
    }
}

/// One log row as the client reads it.
fn log_json(row: &lazypock_db::tokio_postgres::Row) -> Value {
    json!({
        "id": row.get::<_, i64>("id"),
        "method": row.get::<_, Option<String>>("method"),
        "path": row.get::<_, Option<String>>("path"),
        "status": row.get::<_, Option<i32>>("status"),
        "duration": row.get::<_, Option<f64>>("duration"),
        "ip": row.get::<_, Option<String>>("ip"),
        "user_agent": row.get::<_, Option<String>>("user_agent"),
        "referer": row.get::<_, Option<String>>("referer"),
        "collection": row.get::<_, Option<String>>("collection"),
        "error": row.get::<_, Option<String>>("error"),
        "body": row.get::<_, Option<String>>("body"),
        "created_at": row.get::<_, Option<String>>("created_at"),
    })
}

/// `GET /api/logs/stats` query.
#[derive(Debug, Default, Deserialize)]
pub struct StatsQuery {
    /// One of `24h`, `7d`, `30d`.
    #[serde(default)]
    pub range: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_keys_are_redacted_recursively() {
        let body = json!({
            "email": "a@example.com",
            "password": "hunter2",
            "nested": { "passwordConfirm": "hunter2", "keep": 1 },
            "list": [{ "apiKey": "k" }],
        });
        let redacted = redact(body);
        assert_eq!(redacted["email"], json!("a@example.com"));
        assert_eq!(redacted["password"], json!("[REDACTED]"));
        assert_eq!(redacted["nested"]["passwordConfirm"], json!("[REDACTED]"));
        assert_eq!(redacted["nested"]["keep"], json!(1));
        assert_eq!(redacted["list"][0]["apiKey"], json!("[REDACTED]"));
    }

    #[test]
    fn only_small_json_writes_are_buffered() {
        let json = |body: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
            (headers, Body::from(body.to_string()))
        };

        let (headers, body) = json("{\"a\":1}");
        assert!(capturable_body("POST", &headers, &body));
        let (headers, body) = json("{\"a\":1}");
        assert!(capturable_body("PATCH", &headers, &body));
        let (headers, body) = json("{\"a\":1}");
        assert!(!capturable_body("GET", &headers, &body));
        let (headers, body) = json("{\"a\":1}");
        assert!(!capturable_body("DELETE", &headers, &body));

        let (mut headers, body) = json("{\"a\":1}");
        headers.insert(
            header::CONTENT_TYPE,
            "multipart/form-data; boundary=x".parse().unwrap(),
        );
        assert!(!capturable_body("POST", &headers, &body));

        let (headers, body) = json(&"x".repeat(MAX_BODY_BYTES + 1));
        assert!(!capturable_body("POST", &headers, &body));

        // A body whose length is not known is left alone rather than buffered
        // without a bound.
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        let unknown = Body::from_stream(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(
            bytes::Bytes::from_static(b"{}"),
        )]));
        assert!(!capturable_body("POST", &headers, &unknown));
    }

    #[test]
    fn a_status_filter_accepts_a_class_or_an_exact_code() {
        assert_eq!(status_condition("4xx"), Some(StatusFilter::Range(400, 500)));
        assert_eq!(status_condition("5xx"), Some(StatusFilter::Range(500, 600)));
        assert_eq!(status_condition("404"), Some(StatusFilter::Exact(404)));
        assert_eq!(status_condition(""), None);
        assert_eq!(status_condition("nope"), None);
        assert_eq!(status_condition("4x"), None);
    }

    #[test]
    fn the_collection_comes_from_the_records_path() {
        assert_eq!(
            collection_from_path("/api/collections/posts/records"),
            Some("posts".to_string())
        );
        assert_eq!(
            collection_from_path("/api/collections/posts/records/abc"),
            Some("posts".to_string())
        );
        assert_eq!(collection_from_path("/api/collections"), None);
        assert_eq!(collection_from_path("/api/settings"), None);
    }

    #[test]
    fn the_stats_window_is_a_closed_set() {
        assert_eq!(stats_window(Some("24h")), ("24h", "24 hours", "1 hour"));
        assert_eq!(stats_window(Some("7d")), ("7d", "7 days", "6 hours"));
        assert_eq!(stats_window(Some("30d")), ("30d", "30 days", "1 day"));
        assert_eq!(stats_window(None), ("24h", "24 hours", "1 hour"));
        assert_eq!(
            stats_window(Some("1 year'; drop table _logs; --")),
            ("24h", "24 hours", "1 hour"),
            "nothing from the request may reach the SQL"
        );
    }
}
