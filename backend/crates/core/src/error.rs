//! Application-wide error type.
//!
//! Domain and db layers return `core::Error`; the api layer maps it to HTTP
//! responses. Keep variants coarse-grained here and add detail via messages.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The requested resource does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// The caller sent something invalid.
    #[error("invalid input: {0}")]
    Validation(String),

    /// The operation conflicts with current state.
    #[error("conflict: {0}")]
    Conflict(String),

    /// A Space mutation must wait for usage reconciliation to finish.
    #[error("space usage recalculation is in progress")]
    UsageRecalculationInProgress { retry_after_seconds: u64 },

    /// A dependency (db, external service) failed.
    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    pub fn not_found(msg: impl fmt::Display) -> Self {
        Self::NotFound(msg.to_string())
    }

    pub fn validation(msg: impl fmt::Display) -> Self {
        Self::Validation(msg.to_string())
    }

    pub fn conflict(msg: impl fmt::Display) -> Self {
        Self::Conflict(msg.to_string())
    }

    pub fn usage_recalculation_in_progress(retry_after_seconds: u64) -> Self {
        Self::UsageRecalculationInProgress {
            retry_after_seconds,
        }
    }

    pub fn internal(msg: impl fmt::Display) -> Self {
        Self::Internal(msg.to_string())
    }
}
