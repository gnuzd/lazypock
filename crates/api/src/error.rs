//! API errors → HTTP responses.
//!
//! One error type end to end: `lazypock_core::Error`. `IntoResponse` (axum) cannot be
//! implemented for it here (orphan rule), so `ApiError` is a thin newtype.
//!
//! Body: `{"code": <status>, "message": "...", "data": {}}`, the same keys as
//! `/api/health`. 5xx never exposes the message (core's `Error` is a plain String and
//! can name hosts or SQL); it is logged and replaced.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lazypock_core::Error;
use serde_json::json;

#[derive(Debug)]
pub struct ApiError(pub Error);

pub type ApiResult<T> = std::result::Result<T, ApiError>;

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

impl From<JsonRejection> for ApiError {
    fn from(_: JsonRejection) -> Self {
        ApiError(Error::BadRequest(
            "the request body is not valid JSON".into(),
        ))
    }
}

impl From<QueryRejection> for ApiError {
    fn from(_: QueryRejection) -> Self {
        ApiError(Error::BadRequest("the query string is not valid".into()))
    }
}

impl From<PathRejection> for ApiError {
    fn from(_: PathRejection) -> Self {
        ApiError(Error::BadRequest("the request path is not valid".into()))
    }
}

pub fn envelope(code: u16, message: &str) -> serde_json::Value {
    json!({ "code": code, "message": message, "data": {} })
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = self.0.status_code();
        let status = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let message = if code >= 500 {
            tracing::error!(error = %self.0, "request failed");
            "internal server error".to_string()
        } else {
            self.0.to_string()
        };
        let mut response = (status, Json(envelope(code, &message))).into_response();
        if code == 401 {
            response
                .headers_mut()
                .insert("www-authenticate", "Bearer".parse().expect("static header"));
        }
        response
    }
}

/// 429 with `Retry-After` (core's `Error` has no variant for it).
pub fn too_many_requests(retry_after_secs: u64, message: &str) -> Response {
    let mut r = (StatusCode::TOO_MANY_REQUESTS, Json(envelope(429, message))).into_response();
    if let Ok(v) = retry_after_secs.to_string().parse() {
        r.headers_mut().insert("retry-after", v);
    }
    r
}

/// The fallback for unknown routes.
pub async fn not_found() -> ApiError {
    ApiError(Error::NotFound("route".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    async fn render(e: Error) -> (StatusCode, serde_json::Value, Response) {
        let res = ApiError(e).into_response();
        let status = res.status();
        let (parts, body) = res.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap();
        (
            status,
            json,
            Response::from_parts(parts, axum::body::Body::empty()),
        )
    }

    #[tokio::test]
    async fn client_errors_keep_their_message_and_unauthorized_asks_for_a_bearer_token() {
        let (s, b, _) = render(Error::NotFound("collection posts".into())).await;
        assert_eq!(
            (s, b["code"].as_u64(), b["message"].as_str()),
            (
                StatusCode::NOT_FOUND,
                Some(404),
                Some("not found: collection posts")
            )
        );
        assert_eq!(
            render(Error::Conflict("x".into())).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            render(Error::Forbidden("x".into())).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            render(Error::Validation("x".into())).await.0,
            StatusCode::BAD_REQUEST
        );
        let (s, _, r) = render(Error::Unauthorized("no".into())).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(r.headers()["www-authenticate"], "Bearer");
    }

    #[tokio::test]
    async fn server_errors_hide_details() {
        let (s, b, _) = render(Error::Database("password=hunter2 host=db.internal".into())).await;
        assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(b["message"], "internal server error");
        assert!(!b.to_string().contains("hunter2"));
    }
}
