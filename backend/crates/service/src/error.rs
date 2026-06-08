//! Shared service-layer error.
//!
//! Feature services return this; the api layer maps it to HTTP status codes.

use notegate_core::Error as CoreError;

/// A service-layer failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServiceError {
    /// The requested entity does not exist (or is not visible to the caller).
    #[error("{0}")]
    NotFound(String),
    /// The input failed validation.
    #[error("{0}")]
    InvalidInput(String),
    /// The caller is not permitted to perform this action.
    #[error("{0}")]
    Forbidden(String),
    /// The operation conflicts with current state or a limit.
    #[error("{0}")]
    Conflict(String),
    /// An internal/storage failure.
    #[error("{0}")]
    Internal(String),
}

/// The service-layer result alias.
pub type ServiceResult<T> = Result<T, ServiceError>;

impl From<CoreError> for ServiceError {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::NotFound(message) => Self::NotFound(message),
            CoreError::Validation(message) => Self::InvalidInput(message),
            CoreError::Internal(message) => Self::Internal(message),
        }
    }
}

impl From<notegate_core::validation::ValidationError> for ServiceError {
    fn from(error: notegate_core::validation::ValidationError) -> Self {
        Self::InvalidInput(error.to_string())
    }
}

impl From<crate::cursor::CursorError> for ServiceError {
    fn from(_error: crate::cursor::CursorError) -> Self {
        Self::InvalidInput("invalid cursor".to_owned())
    }
}
