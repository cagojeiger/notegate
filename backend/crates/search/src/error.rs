//! Search-layer failures kept independent from transport and the wider service crate.

use notegate_core::{Error as CoreError, WriteLockScope};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SearchError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{scope}")]
    WriteLocked { scope: WriteLockScope },
    #[error("space usage recalculation is in progress")]
    UsageRecalculationInProgress { retry_after_seconds: u64 },
    #[error("{0}")]
    Internal(String),
}

pub type SearchResult<T> = Result<T, SearchError>;

impl From<CoreError> for SearchError {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::NotFound(message) => Self::NotFound(message),
            CoreError::Validation(message) => Self::InvalidInput(message),
            CoreError::Conflict(message) => Self::Conflict(message),
            CoreError::WriteLocked { scope } => Self::WriteLocked { scope },
            CoreError::UsageRecalculationInProgress {
                retry_after_seconds,
            } => Self::UsageRecalculationInProgress {
                retry_after_seconds,
            },
            CoreError::Internal(message) => Self::Internal(message),
        }
    }
}

impl From<notegate_core::validation::ValidationError> for SearchError {
    fn from(error: notegate_core::validation::ValidationError) -> Self {
        Self::InvalidInput(error.to_string())
    }
}

impl From<notegate_core::cursor::CursorError> for SearchError {
    fn from(_error: notegate_core::cursor::CursorError) -> Self {
        Self::InvalidInput("invalid cursor".to_owned())
    }
}
