use reqwest::StatusCode;
use serde_json::{Value, json};
use std::fmt;

pub const EXIT_INVALID_INPUT: u8 = 2;
pub const EXIT_AUTH: u8 = 3;
pub const EXIT_COMMAND_REJECTED: u8 = 4;
pub const EXIT_UNAVAILABLE: u8 = 5;

#[derive(Debug)]
pub struct CliError {
    exit_code: u8,
    body: Value,
}

impl CliError {
    pub fn invalid_input(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(EXIT_INVALID_INPUT, code, "invalid_input", message, false)
    }

    pub fn configuration(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(
            EXIT_INVALID_INPUT,
            code,
            "configuration_error",
            message,
            false,
        )
    }

    pub fn auth(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(EXIT_AUTH, code, "authentication_error", message, false)
    }

    pub fn retryable_auth(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(
            EXIT_UNAVAILABLE,
            code,
            "authentication_error",
            message,
            true,
        )
    }

    pub fn unavailable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(EXIT_UNAVAILABLE, code, "transport_error", message, true)
    }

    pub fn protocol(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(EXIT_UNAVAILABLE, code, "protocol_error", message, true)
    }

    pub fn recoverable_protocol(
        code: &'static str,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            exit_code: EXIT_UNAVAILABLE,
            body: json!({
                "error": code,
                "kind": "protocol_error",
                "message": message.into(),
                "data": {
                    "retryable": false,
                    "recoverable": true,
                    "hint": hint.into(),
                },
            }),
        }
    }

    pub fn server(status: StatusCode, body: Value) -> Self {
        let exit_code = if body.get("error").and_then(Value::as_str) == Some("cli_update_required")
        {
            EXIT_COMMAND_REJECTED
        } else if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            EXIT_AUTH
        } else if matches!(
            status,
            StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS
        ) || status.is_server_error()
        {
            EXIT_UNAVAILABLE
        } else {
            EXIT_COMMAND_REJECTED
        };
        Self { exit_code, body }
    }

    pub const fn exit_code(&self) -> u8 {
        self.exit_code
    }

    pub const fn body(&self) -> &Value {
        &self.body
    }

    fn new(
        exit_code: u8,
        code: &'static str,
        kind: &'static str,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            exit_code,
            body: json!({
                "error": code,
                "kind": kind,
                "message": message.into(),
                "data": {
                    "retryable": retryable,
                },
            }),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.body)
    }
}

impl std::error::Error for CliError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_statuses_map_to_stable_exit_codes_without_rewriting_the_body() {
        let body = json!({
            "error": "required_field_missing",
            "data": {"next_action": {"kind": "add_fields"}},
        });

        for (status, exit_code) in [
            (StatusCode::UNAUTHORIZED, EXIT_AUTH),
            (StatusCode::BAD_REQUEST, EXIT_COMMAND_REJECTED),
            (StatusCode::REQUEST_TIMEOUT, EXIT_UNAVAILABLE),
            (StatusCode::TOO_MANY_REQUESTS, EXIT_UNAVAILABLE),
            (StatusCode::SERVICE_UNAVAILABLE, EXIT_UNAVAILABLE),
        ] {
            let error = CliError::server(status, body.clone());
            assert_eq!(error.exit_code(), exit_code);
            assert_eq!(error.body(), &body);
        }
    }

    #[test]
    fn update_required_is_a_structured_command_rejection_regardless_of_status() {
        let body = json!({
            "error": "cli_update_required",
            "kind": "client_version_incompatible",
            "message": "update notegate-cli before retrying",
            "data": {
                "client_version": "0.1.79",
                "server_version": "0.1.80",
                "next_action": {"kind": "run_command", "command": "notegate-cli update"},
            },
        });

        let error = CliError::server(StatusCode::UNAUTHORIZED, body.clone());
        assert_eq!(error.exit_code(), EXIT_COMMAND_REJECTED);
        assert_eq!(error.body(), &body);
    }
}
