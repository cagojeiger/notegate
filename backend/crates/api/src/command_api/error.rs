use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use notegate_command::{COMMAND_PROTOCOL_VERSION, CommandError, CommandErrorClass, RecoveryAction};
use serde::Serialize;
use serde_json::{Value, json};

/// Stable HTTP representation of a transport-neutral command failure.
#[derive(Debug, Serialize)]
pub(super) struct CommandErrorBody {
    pub(super) error: String,
    pub(super) kind: String,
    pub(super) message: String,
    pub(super) data: Option<Value>,
}

#[derive(Debug)]
pub(super) struct CommandHttpError {
    status: StatusCode,
    body: CommandErrorBody,
}

impl CommandHttpError {
    pub(super) fn invalid_json(error: JsonRejection) -> Self {
        Self::invalid_json_detail(error.body_text())
    }

    pub(super) fn invalid_schema(error: serde_json::Error) -> Self {
        Self::invalid_json_detail(error.to_string())
    }

    pub(super) fn cli_update_required(
        client_version: Option<&str>,
        client_protocol_version: Option<&str>,
    ) -> Self {
        Self {
            status: StatusCode::UPGRADE_REQUIRED,
            body: CommandErrorBody {
                error: "cli_update_required".to_owned(),
                kind: "client_protocol_incompatible".to_owned(),
                message: "update notegate-cli before retrying".to_owned(),
                data: Some(json!({
                    "kind": "client_protocol_incompatible",
                    "code": "cli_update_required",
                    "client_version": client_version,
                    "server_version": env!("CARGO_PKG_VERSION"),
                    "client_protocol_version": client_protocol_version,
                    "server_protocol_version": COMMAND_PROTOCOL_VERSION,
                    "retryable": false,
                    "recoverable": true,
                    "next_action": RecoveryAction::RunCommand {
                        command: "notegate-cli update".to_owned(),
                    },
                })),
            },
        }
    }

    fn invalid_json_detail(detail: String) -> Self {
        CommandError::invalid_params("request body is invalid")
            .with_data(json!({
                "kind": "invalid_input",
                "code": "invalid_json",
                "detail": detail,
            }))
            .into()
    }

    pub(super) fn error_code(&self) -> &str {
        &self.body.error
    }

    pub(super) fn kind(&self) -> &str {
        &self.body.kind
    }

    pub(super) fn data(&self) -> Option<&Value> {
        self.body.data.as_ref()
    }

    #[cfg(test)]
    pub(super) fn status(&self) -> StatusCode {
        self.status
    }

    #[cfg(test)]
    pub(super) fn body(&self) -> &CommandErrorBody {
        &self.body
    }
}

impl From<CommandError> for CommandHttpError {
    fn from(error: CommandError) -> Self {
        let kind = string_field(error.data.as_ref(), "kind")
            .unwrap_or_else(|| default_kind(error.class))
            .to_owned();
        let code = string_field(error.data.as_ref(), "code")
            .unwrap_or(kind.as_str())
            .to_owned();
        let status = status_for(error.class, &kind, &code);

        Self {
            status,
            body: CommandErrorBody {
                error: code,
                kind,
                message: error.message,
                data: error.data,
            },
        }
    }
}

impl IntoResponse for CommandHttpError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

fn string_field<'a>(data: Option<&'a Value>, field: &str) -> Option<&'a str> {
    data.and_then(|value| value.get(field))
        .and_then(Value::as_str)
}

fn default_kind(class: CommandErrorClass) -> &'static str {
    match class {
        CommandErrorClass::InvalidParams | CommandErrorClass::InvalidRequest => "invalid_input",
        CommandErrorClass::TemporaryUnavailable => "temporary_unavailable",
        CommandErrorClass::CapacityBusy => "capacity_busy",
        CommandErrorClass::Internal => "internal_error",
    }
}

fn status_for(class: CommandErrorClass, kind: &str, code: &str) -> StatusCode {
    match (kind, code) {
        ("not_found", _) => StatusCode::NOT_FOUND,
        ("forbidden", _) => StatusCode::FORBIDDEN,
        ("conflict", _) => StatusCode::CONFLICT,
        ("write_locked", _) | (_, "node_write_locked" | "subtree_write_locked") => {
            StatusCode::LOCKED
        }
        ("search_busy", _) => StatusCode::TOO_MANY_REQUESTS,
        (
            "search_unavailable"
            | "deadline_exceeded"
            | "temporary_unavailable"
            | "usage_recalculation_in_progress",
            _,
        ) => StatusCode::SERVICE_UNAVAILABLE,
        ("internal_error", _) => StatusCode::INTERNAL_SERVER_ERROR,
        _ => match class {
            CommandErrorClass::CapacityBusy => StatusCode::TOO_MANY_REQUESTS,
            CommandErrorClass::TemporaryUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            CommandErrorClass::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            CommandErrorClass::InvalidParams | CommandErrorClass::InvalidRequest => {
                StatusCode::BAD_REQUEST
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_kind_selects_status_and_preserves_recovery_data() {
        let data = json!({
            "kind": "write_locked",
            "code": "node_write_locked",
            "hint": "unlock the target",
            "next_action": {"kind": "remove_fields", "fields": ["force"]},
        });
        let mapped = CommandHttpError::from(
            CommandError::invalid_request("target is locked").with_data(data.clone()),
        );

        assert_eq!(mapped.status(), StatusCode::LOCKED);
        assert_eq!(mapped.body().error, "node_write_locked");
        assert_eq!(mapped.body().kind, "write_locked");
        assert_eq!(mapped.body().message, "target is locked");
        assert_eq!(mapped.body().data, Some(data));
    }

    #[test]
    fn error_classes_supply_status_and_stable_fallback_kind() {
        let cases = [
            (
                CommandError::capacity_busy("busy"),
                StatusCode::TOO_MANY_REQUESTS,
                "capacity_busy",
            ),
            (
                CommandError::temporary_unavailable("retry"),
                StatusCode::SERVICE_UNAVAILABLE,
                "temporary_unavailable",
            ),
            (
                CommandError::internal("failed"),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            ),
            (
                CommandError::invalid_params("bad input"),
                StatusCode::BAD_REQUEST,
                "invalid_input",
            ),
        ];

        for (error, status, kind) in cases {
            let mapped = CommandHttpError::from(error);
            assert_eq!(mapped.status(), status);
            assert_eq!(mapped.body().error, kind);
            assert_eq!(mapped.body().kind, kind);
            assert!(mapped.body().data.is_none());
        }
    }

    #[test]
    fn semantic_statuses_override_generic_command_classes() {
        for (kind, expected) in [
            ("not_found", StatusCode::NOT_FOUND),
            ("forbidden", StatusCode::FORBIDDEN),
            ("conflict", StatusCode::CONFLICT),
            ("search_busy", StatusCode::TOO_MANY_REQUESTS),
            ("search_unavailable", StatusCode::SERVICE_UNAVAILABLE),
        ] {
            let mapped =
                CommandHttpError::from(CommandError::invalid_params(kind).with_data(json!({
                    "kind": kind,
                    "code": kind,
                })));
            assert_eq!(mapped.status(), expected, "{kind}");
        }
    }
}
