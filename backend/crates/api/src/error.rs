//! HTTP error type. Domain/db/service errors map into this on their way out.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use notegate_core::Error as CoreError;
use notegate_service::ServiceError;
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_input", message)
    }

    pub fn invalid_field(message: impl Into<String>) -> Self {
        Self::invalid_input(message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
    }
}

/// Map a service-layer error to an HTTP response per `docs/spec/rest/errors.md`.
/// Internal failures are logged but their detail is never returned to the client.
impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::NotFound(message) => Self::not_found(message),
            ServiceError::InvalidInput(message) => Self::invalid_field(message),
            ServiceError::Forbidden(message) => Self::forbidden(message),
            ServiceError::Conflict(message) => Self::conflict(message),
            ServiceError::Internal(message) => {
                tracing::error!(event = "error.internal", detail = %message);
                Self::internal("internal server error")
            }
        }
    }
}

/// Map the domain error to an HTTP response. Internal details are logged but
/// never leaked to the client.
impl From<CoreError> for ApiError {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::NotFound(msg) => Self::not_found(msg),
            CoreError::Validation(msg) => Self::invalid_field(msg),
            CoreError::Conflict(msg) => Self::conflict(msg),
            CoreError::Internal(msg) => {
                tracing::error!(event = "error.internal", detail = %msg);
                Self::internal("internal server error")
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.code,
                "kind": self.code,
                "message": self.message,
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_field_uses_common_invalid_input_kind() {
        let error = ApiError::invalid_field("bad field");
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.code, "invalid_input");
    }
}
