//! Mapping from application errors to the PocketBase error envelope.
//!
//! Every error response has the shape:
//! `{"status": 404, "message": "...", "data": {}}`.
//!
//! Internal failures never leak their message to the client; the real error is
//! logged instead.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use lazypock_core::error::Error;
use serde::Serialize;

/// PocketBase-compatible error body.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// HTTP status code, repeated in the body (PocketBase contract).
    pub status: u16,
    /// Human-readable message.
    pub message: String,
    /// Per-field validation details; always present, empty for generic errors.
    pub data: serde_json::Value,
}

/// Error wrapper that renders as an HTTP response.
#[derive(Debug)]
pub struct ApiError(pub Error);

impl ApiError {
    /// Build an `ApiError` from anything convertible into a [`Error`].
    pub fn new(error: impl Into<Error>) -> Self {
        Self(error.into())
    }
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        let message = match &self.0 {
            // Do not leak internal details or configuration to clients.
            Error::Internal(message) => {
                tracing::error!(error = %message, "internal error");
                "Something went wrong while processing the request.".to_string()
            }
            Error::Config(message) => {
                tracing::error!(error = %message, "configuration error reached the request path");
                "The server is misconfigured.".to_string()
            }
            other => other.message().to_string(),
        };

        let body = ErrorBody {
            status: status.as_u16(),
            message,
            data: serde_json::json!({}),
        };

        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn render(error: Error) -> (StatusCode, serde_json::Value) {
        let response = ApiError::new(error).into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn client_errors_keep_their_message() {
        let (status, body) = render(Error::not_found("No record found.")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["status"], 404);
        assert_eq!(body["message"], "No record found.");
        assert!(body["data"].is_object());
    }

    #[tokio::test]
    async fn internal_errors_do_not_leak_details() {
        let (status, body) = render(Error::internal("connection string postgres://u:p@h/db")).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            !body["message"].as_str().unwrap().contains("postgres://"),
            "internal detail leaked: {body}"
        );
    }
}
