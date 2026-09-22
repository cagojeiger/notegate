//! Shared service-layer error.
//!
//! Feature services return this; the api layer maps it to HTTP status codes.

use notegate_core::{Error as CoreError, WriteLockScope};

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
    /// A direct or inherited node write lock blocks the mutation.
    #[error("{scope}")]
    WriteLocked { scope: WriteLockScope },
    /// A Space mutation must be retried after usage reconciliation finishes.
    #[error("space usage recalculation is in progress")]
    UsageRecalculationInProgress { retry_after_seconds: u64 },
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

impl From<notegate_text::patch::PatchError> for ServiceError {
    fn from(error: notegate_text::patch::PatchError) -> Self {
        use notegate_text::patch::PatchError;

        match error {
            PatchError::EmptyOldText => {
                ServiceError::InvalidInput("edit old_text must not be empty".to_owned())
            }
            PatchError::NoOpEdit => ServiceError::InvalidInput(
                "edit old_text and new_text are identical (no-op)".to_owned(),
            ),
            PatchError::InvalidLine(message) => ServiceError::InvalidInput(message),
            PatchError::NoMatch => ServiceError::Conflict(
                "old_text did not match the current text; read it again before patching".to_owned(),
            ),
            PatchError::MultipleMatches => ServiceError::Conflict(
                "old_text matched multiple times; use mode='all' or include more surrounding context"
                    .to_owned(),
            ),
            PatchError::CountMismatch { expected, actual } => ServiceError::Conflict(format!(
                "expected_count was {expected}, but current text has {actual} matches"
            )),
            PatchError::OverlappingEdits => {
                ServiceError::Conflict("edits target overlapping ranges".to_owned())
            }
        }
    }
}

impl From<notegate_text::format::FormatError> for ServiceError {
    fn from(error: notegate_text::format::FormatError) -> Self {
        Self::InvalidInput(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use notegate_text::patch::{PatchError, apply_edits, apply_line_edits};
    use notegate_text::{Edit, LineEdit, PatchMode};

    use super::*;

    #[test]
    fn text_engine_errors_keep_input_and_conflict_classification() {
        let edit = Edit {
            old_text: "missing".to_owned(),
            new_text: "replacement".to_owned(),
            mode: PatchMode::Unique,
            expected_count: None,
        };
        let missing = apply_edits("current text", &[edit]).map_err(ServiceError::from);
        assert_eq!(
            missing,
            Err(ServiceError::Conflict(
                "old_text did not match the current text; read it again before patching".to_owned()
            )),
        );
        let invalid_line = apply_line_edits(
            "one line\n",
            &[LineEdit::DeleteLines {
                start_line: 2,
                end_line: 2,
            }],
        )
        .map_err(ServiceError::from);
        assert_eq!(
            invalid_line,
            Err(ServiceError::InvalidInput(
                "line must be between 1 and 1".to_owned()
            )),
        );
        assert_eq!(
            ServiceError::from(PatchError::CountMismatch {
                expected: 2,
                actual: 1
            }),
            ServiceError::Conflict(
                "expected_count was 2, but current text has 1 matches".to_owned()
            ),
        );
    }

    #[test]
    fn structured_syntax_errors_keep_the_original_message() {
        let result = crate::files::validate_structured_text("events.jsonl", "{}\n\n");
        assert_eq!(
            result,
            Err(ServiceError::InvalidInput(
                "invalid jsonl syntax in events.jsonl at line 2, column 1: blank lines are not valid JSONL records".to_owned()
            )),
        );
    }
}
