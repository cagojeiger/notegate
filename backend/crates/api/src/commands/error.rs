//! Transport-neutral command error construction and service error mapping.

use notegate_command::{CommandError, RecoveryAction, RecoveryErrorData, RequiredField};
use notegate_core::WriteLockScope;
use notegate_search::SearchError;
use notegate_service::ServiceError;
use serde_json::json;

use crate::error::write_lock_code;

/// Map a service-layer error to the shared command error contract.
pub fn service_error(error: ServiceError) -> CommandError {
    match error {
        ServiceError::NotFound(message) => {
            CommandError::invalid_params(message).with_data(error_meta("not_found"))
        }
        ServiceError::InvalidInput(message) => {
            CommandError::invalid_params(message).with_data(error_meta("invalid_input"))
        }
        ServiceError::Forbidden(message) => {
            CommandError::invalid_request(message).with_data(error_meta("forbidden"))
        }
        ServiceError::Conflict(message) => {
            CommandError::invalid_request(message).with_data(error_meta("conflict"))
        }
        ServiceError::WriteLocked { scope } => write_locked_error(scope),
        ServiceError::UsageRecalculationInProgress {
            retry_after_seconds,
        } => {
            CommandError::temporary_unavailable("space usage is being recalculated; retry shortly")
                .with_data(json!({
                    "kind": "usage_recalculation_in_progress",
                    "code": "usage_recalculation_in_progress",
                    "retryable": true,
                    "retry_after_seconds": retry_after_seconds,
                }))
        }
        ServiceError::Internal(message) => {
            tracing::error!(event = "command.error.internal", detail = %message);
            CommandError::internal("internal server error").with_data(error_meta("internal_error"))
        }
    }
}

/// Map a search-layer error through the same public contract as service failures.
pub fn search_error(error: SearchError) -> CommandError {
    let error = match error {
        SearchError::NotFound(message) => ServiceError::NotFound(message),
        SearchError::InvalidInput(message) => ServiceError::InvalidInput(message),
        SearchError::Forbidden(message) => ServiceError::Forbidden(message),
        SearchError::Conflict(message) => ServiceError::Conflict(message),
        SearchError::WriteLocked { scope } => ServiceError::WriteLocked { scope },
        SearchError::UsageRecalculationInProgress {
            retry_after_seconds,
        } => ServiceError::UsageRecalculationInProgress {
            retry_after_seconds,
        },
        SearchError::Internal(message) => ServiceError::Internal(message),
    };
    service_error(error)
}

fn write_locked_error(scope: WriteLockScope) -> CommandError {
    let (scope_name, hint) = match scope {
        WriteLockScope::TargetOrAncestor => (
            "target_or_ancestor",
            "Use read op=stat on the target to inspect write_lock_sources. Only the space owner can unlock it in the Dashboard. If file_upload begin_upload was rejected, unlock the target and call begin_upload again; no upload handle was created.",
        ),
        WriteLockScope::Descendant => (
            "descendant",
            "Inspect the subtree for direct write locks. Only the space owner can unlock them in the Dashboard.",
        ),
    };
    CommandError::invalid_request(scope.to_string()).with_data(json!({
        "kind": "write_locked",
        "code": write_lock_code(scope),
        "scope": scope_name,
        "retryable": false,
        "hint": hint,
    }))
}

fn error_meta(kind: &'static str) -> serde_json::Value {
    RecoveryErrorData::basic(kind, kind).into_value()
}

pub fn invalid_input_error(message: impl Into<String>) -> CommandError {
    CommandError::invalid_params(message).with_data(error_meta("invalid_input"))
}

/// Enforce the purpose invariant shared by every command transport.
pub fn validate_purpose(purpose: &str) -> Result<(), CommandError> {
    notegate_command::validate_purpose(purpose)
        .map_err(|error| invalid_input_error(error.to_string()))
}

/// Build an invalid-input error that a caller can correct without parsing the
/// human-readable message.
pub fn actionable_input_error(
    code: &'static str,
    message: impl Into<String>,
    hint: &'static str,
    next_action: RecoveryAction,
) -> CommandError {
    CommandError::invalid_params(message)
        .with_data(RecoveryErrorData::actionable_input(code, hint, next_action).into_value())
}

/// Require an operation-specific field that cannot be globally required in a
/// unified command schema.
pub fn required_input<T>(value: Option<T>, field: &str, context: &str) -> Result<T, CommandError> {
    value.ok_or_else(|| {
        actionable_input_error(
            "required_field_missing",
            format!("{context} requires {field}; retry with field `{field}` set"),
            "Add the field described by next_action.fields and retry the same tool.",
            RecoveryAction::AddFields {
                fields: vec![RequiredField {
                    field: field.to_owned(),
                    description: None,
                }],
            },
        )
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use notegate_command::CommandErrorClass;

    use super::*;

    #[test]
    fn service_errors_keep_the_structured_contract() {
        let missing = service_error(ServiceError::NotFound("missing".to_owned()));
        assert_eq!(missing.class, CommandErrorClass::InvalidParams);
        let missing_data = missing.data.expect("not_found carries data");
        assert_eq!(missing_data["kind"], "not_found");
        assert_eq!(missing_data["code"], "not_found");

        let forbidden = service_error(ServiceError::Forbidden("no".to_owned()));
        assert_eq!(forbidden.class, CommandErrorClass::InvalidRequest);
        let forbidden_data = forbidden.data.expect("forbidden carries data");
        assert_eq!(forbidden_data["kind"], "forbidden");
        assert_eq!(forbidden_data["code"], "forbidden");

        let locked = service_error(ServiceError::WriteLocked {
            scope: WriteLockScope::TargetOrAncestor,
        });
        assert_eq!(locked.class, CommandErrorClass::InvalidRequest);
        let locked_data = locked.data.expect("write lock carries data");
        assert_eq!(locked_data["kind"], "write_locked");
        assert_eq!(locked_data["code"], "node_write_locked");
        assert_eq!(locked_data["scope"], "target_or_ancestor");
        assert_eq!(locked_data["retryable"], false);
        assert!(
            locked_data["hint"]
                .as_str()
                .is_some_and(|hint| hint.contains("begin_upload"))
        );

        let internal = service_error(ServiceError::Internal("db detail".to_owned()));
        assert_eq!(internal.class, CommandErrorClass::Internal);
        assert_eq!(internal.message, "internal server error");
        let internal_data = internal.data.expect("internal_error carries data");
        assert_eq!(internal_data["kind"], "internal_error");
        assert_eq!(internal_data["code"], "internal_error");
    }

    #[test]
    fn search_errors_preserve_the_service_error_contract() {
        let cases = [
            (
                SearchError::NotFound("missing".to_owned()),
                ServiceError::NotFound("missing".to_owned()),
            ),
            (
                SearchError::InvalidInput("bad".to_owned()),
                ServiceError::InvalidInput("bad".to_owned()),
            ),
            (
                SearchError::Forbidden("no".to_owned()),
                ServiceError::Forbidden("no".to_owned()),
            ),
            (
                SearchError::Conflict("stale".to_owned()),
                ServiceError::Conflict("stale".to_owned()),
            ),
            (
                SearchError::WriteLocked {
                    scope: WriteLockScope::TargetOrAncestor,
                },
                ServiceError::WriteLocked {
                    scope: WriteLockScope::TargetOrAncestor,
                },
            ),
            (
                SearchError::UsageRecalculationInProgress {
                    retry_after_seconds: 5,
                },
                ServiceError::UsageRecalculationInProgress {
                    retry_after_seconds: 5,
                },
            ),
            (
                SearchError::Internal("detail".to_owned()),
                ServiceError::Internal("detail".to_owned()),
            ),
        ];

        for (search, service) in cases {
            assert_eq!(search_error(search), service_error(service));
        }
    }

    #[test]
    fn actionable_input_error_keeps_recovery_metadata() {
        let error = actionable_input_error(
            "field_not_allowed",
            "field is not allowed",
            "Remove the field and retry.",
            RecoveryAction::RemoveFields {
                fields: vec!["field".to_owned()],
            },
        );

        assert_eq!(error.class, CommandErrorClass::InvalidParams);
        let data = error.data.expect("actionable input error carries data");
        assert_eq!(data["kind"], "invalid_input");
        assert_eq!(data["code"], "field_not_allowed");
        assert_eq!(data["retryable"], false);
        assert_eq!(data["recoverable"], true);
        assert_eq!(data["hint"], "Remove the field and retry.");
        assert_eq!(data["next_action"]["kind"], "remove_fields");
    }

    #[test]
    fn purpose_validation_uses_the_shared_command_contract() {
        for purpose in ["", " padded "] {
            let error = validate_purpose(purpose).expect_err("invalid purpose is rejected");
            assert_eq!(error.class, CommandErrorClass::InvalidParams);
            assert_eq!(
                error.data.expect("invalid purpose carries metadata")["kind"],
                "invalid_input"
            );
        }

        let error = validate_purpose(&"가".repeat(notegate_command::PURPOSE_MAX_CHARS + 1))
            .expect_err("overlong purpose is rejected");
        assert_eq!(error.class, CommandErrorClass::InvalidParams);
    }

    #[test]
    fn usage_recalculation_is_retryable() {
        let error = service_error(ServiceError::UsageRecalculationInProgress {
            retry_after_seconds: 5,
        });
        assert_eq!(error.class, CommandErrorClass::TemporaryUnavailable);
        let data = error.data.expect("temporary error carries retry metadata");
        assert_eq!(data["kind"], "usage_recalculation_in_progress");
        assert_eq!(data["retryable"], true);
        assert_eq!(data["retry_after_seconds"], 5);
    }
}
