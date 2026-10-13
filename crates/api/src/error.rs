//! Mapping application errors to the PocketBase error envelope.
//!
//! Every error response has the shape:
//! `{"status": 404, "message": "...", "data": {}}`.
//!
//! `data` carries per-field validation details when there are any, which is how
//! PocketBase reports a failed write.
//!
//! Internal failures never leak their message to the client; the real error is
//! logged instead.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use lazypock_core::error::Error;
use lazypock_db::records::RecordWriteError;
use serde::Serialize;
use serde_json::Value;

/// PocketBase-compatible error body.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// HTTP status code, repeated in the body (PocketBase contract).
    pub status: u16,
    /// Human-readable message.
    pub message: String,
    /// Per-field validation details; always present, empty for generic errors.
    pub data: Value,
}

/// Error wrapper that renders as an HTTP response.
#[derive(Debug)]
pub struct ApiError {
    /// The underlying error.
    pub error: Error,
    /// Per-field details, empty for generic errors.
    pub data: Value,
}

impl ApiError {
    /// Build an `ApiError` from anything convertible into an [`Error`].
    pub fn new(error: impl Into<Error>) -> Self {
        Self {
            error: error.into(),
            data: Value::Object(serde_json::Map::new()),
        }
    }

    /// Build an `ApiError` that carries per-field details.
    pub fn with_data(error: impl Into<Error>, data: Value) -> Self {
        Self {
            error: error.into(),
            data,
        }
    }

    /// Prefix the message, keeping the status and any per-field details.
    pub fn context(self, context: impl std::fmt::Display) -> Self {
        Self {
            error: self.error.context(context),
            data: self.data,
        }
    }
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self::new(error)
    }
}

impl From<RecordWriteError> for ApiError {
    fn from(error: RecordWriteError) -> Self {
        match error {
            RecordWriteError::Validation(errors) => {
                let message = format!("Failed to save the record. {}", errors.summary());
                Self::with_data(Error::invalid(message), errors.to_json())
            }
            RecordWriteError::Other(error) => Self::new(error),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.error.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        let message = match &self.error {
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
            data: self.data,
        };

        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use lazypock_db::validation::ValidationErrors;

    async fn render(error: ApiError) -> (StatusCode, serde_json::Value) {
        let response = error.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn client_errors_keep_their_message() {
        let (status, body) = render(ApiError::new(Error::not_found("No record found."))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["status"], 404);
        assert_eq!(body["message"], "No record found.");
        assert!(body["data"].is_object());
    }

    #[tokio::test]
    async fn internal_errors_do_not_leak_details() {
        let (status, body) = render(ApiError::new(Error::internal(
            "connection string postgres://u:p@h/db",
        )))
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            !body["message"].as_str().unwrap().contains("postgres://"),
            "internal detail leaked: {body}"
        );
    }

    #[tokio::test]
    async fn validation_failures_carry_per_field_details() {
        let mut errors = ValidationErrors::default();
        errors.insert("title", "validation_required", "title is required.");
        errors.insert(
            "views",
            "validation_min_number",
            "views must be at least 0.",
        );

        let (status, body) = render(ApiError::from(RecordWriteError::Validation(errors))).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["status"], 400);
        assert_eq!(body["data"]["title"]["code"], "validation_required");
        assert_eq!(body["data"]["views"]["code"], "validation_min_number");
        assert!(body["message"].as_str().unwrap().contains("title"));
    }
}
