//! Cross-origin requests, driven by the `cors_origins` setting.
//!
//! Studio lets an operator edit the allowed origins and then calls
//! `POST /api/settings/refresh-cors` to apply them "immediately". That only means
//! anything if CORS is actually enforced somewhere — an endpoint that accepts the
//! call and changes nothing is worse than a 404, because it reports success. So this
//! module is the enforcement, and the refresh endpoint reloads it.
//!
//! The allowlist is held in a swap cell rather than baked into a layer at startup,
//! so a change takes effect on the next request without a restart — which is the
//! behaviour the refresh call is asking for.

use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::extract::{Request, State};
use axum::http::{header, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use lazypock_db::tokio_postgres::Client;

use lazypock_core::error::Result;

use crate::state::AppState;

/// The `_settings` key the allowlist lives under.
pub const SETTINGS_KEY: &str = "cors_origins";

/// The value that allows any origin.
const ANY: &str = "*";

/// The methods a browser is told it may use.
const ALLOWED_METHODS: &str = "GET, POST, PATCH, DELETE, OPTIONS";

/// How long a browser may cache a preflight result.
const MAX_AGE: &str = "600";

/// The allowed origins, replaceable without a restart.
#[derive(Debug)]
pub struct CorsPolicy {
    origins: ArcSwap<Vec<String>>,
}

impl Default for CorsPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl CorsPolicy {
    /// A policy that allows nothing until it is loaded.
    ///
    /// Closed by default: a request that arrives before the settings load gets no
    /// CORS headers, which is the same treatment as one from a disallowed origin.
    pub fn new() -> Self {
        Self {
            origins: ArcSwap::from_pointee(Vec::new()),
        }
    }

    /// Load the allowlist from settings.
    pub async fn load(&self, client: &Client) -> Result<()> {
        let row = client
            .query_opt(
                "select value from _settings where key = $1::text",
                &[&SETTINGS_KEY.to_string()],
            )
            .await
            .map_err(lazypock_db::pool::db_error)?;

        // Absent, or not a list, means nothing is allowed. A settings value that
        // cannot be read as a list is not something to guess at.
        let origins = row
            .map(|row| row.get::<_, serde_json::Value>("value"))
            .and_then(|value| value.as_array().cloned())
            .map(|list| {
                list.iter()
                    .filter_map(|entry| entry.as_str())
                    .map(|entry| entry.trim().trim_end_matches('/').to_string())
                    .filter(|entry| !entry.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        tracing::info!(origins = ?origins, "loaded the CORS allowlist");
        self.origins.store(Arc::new(origins));
        Ok(())
    }

    /// The origins currently allowed.
    pub fn origins(&self) -> Vec<String> {
        self.origins.load().as_ref().clone()
    }

    /// Whether an origin is allowed.
    pub fn allows(&self, origin: &str) -> bool {
        let candidate = origin.trim().trim_end_matches('/');
        let allowed = self.origins.load();
        allowed
            .iter()
            .any(|entry| entry == ANY || entry.eq_ignore_ascii_case(candidate))
    }
}

/// Apply CORS to every request.
///
/// A disallowed origin gets a normal response with no CORS headers, which is what
/// the browser needs to block it — refusing the request outright would turn a
/// cross-origin read into an error the application cannot distinguish from a real
/// failure.
pub async fn apply(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let allowed = origin.as_deref().filter(|origin| state.cors.allows(origin));

    // Read before the request is handed on, since it does not survive the move.
    let requested_headers = request
        .headers()
        .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .cloned();

    // A preflight is answered here rather than by the router, because the route it
    // names may not accept OPTIONS at all.
    if request.method() == Method::OPTIONS {
        let mut response = if allowed.is_some() {
            StatusCode::NO_CONTENT.into_response()
        } else {
            // No CORS headers, so the browser fails the preflight.
            StatusCode::NO_CONTENT.into_response()
        };
        if let Some(origin) = allowed {
            decorate(response.headers_mut(), origin, requested_headers.as_ref());
        } else {
            vary(response.headers_mut());
        }
        return response;
    }

    let mut response = next.run(request).await;
    // `Vary` is set either way: the response differs by origin, and a shared cache
    // that misses that will serve one origin's headers to another.
    vary(response.headers_mut());
    if let Some(origin) = allowed {
        decorate(response.headers_mut(), origin, requested_headers.as_ref());
    }
    response
}

/// Tell caches the response depends on the origin.
fn vary(headers: &mut axum::http::HeaderMap) {
    headers.append(header::VARY, HeaderValue::from_static("Origin"));
}

/// Add the CORS headers for an allowed origin.
fn decorate(
    headers: &mut axum::http::HeaderMap,
    origin: &str,
    requested_headers: Option<&HeaderValue>,
) {
    // The origin is echoed rather than answered with `*`, because `*` is invalid
    // alongside credentials — a client that sends an Authorization header needs the
    // specific origin back.
    if let Ok(value) = HeaderValue::from_str(origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static(ALLOWED_METHODS),
    );
    // The headers the client asked for, so a new request header does not need a
    // server change to start working.
    let requested = requested_headers
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("content-type, authorization"));
    headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, requested);
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("true"),
    );
    headers.insert(
        HeaderName::from_static("access-control-max-age"),
        HeaderValue::from_static(MAX_AGE),
    );
}
