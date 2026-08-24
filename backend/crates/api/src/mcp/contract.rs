//! Shared structured contracts for MCP recovery instructions and errors.

use notegate_command::{CommandError, CommandErrorClass};
pub use notegate_command::{
    RecoveryAction as McpAction, RecoveryErrorData as McpErrorData, RequiredField,
};
use rmcp::ErrorData;
use rmcp::model::ErrorCode;
use serde_json::{Value, json};

/// Retryable dependency/maintenance failure shared by MCP tools.
pub const TEMPORARY_UNAVAILABLE_ERROR_CODE: ErrorCode = ErrorCode(-32001);
/// Process-local capacity rejection shared by MCP tools.
pub const CAPACITY_BUSY_ERROR_CODE: ErrorCode = ErrorCode(-32002);

pub fn error_json(error: ErrorData) -> Value {
    json!({
        "code": error.code.0,
        "message": error.message,
        "data": error.data,
    })
}

/// Convert the transport-neutral command failure into the unchanged MCP error
/// envelope at the protocol boundary.
pub fn command_error(error: CommandError) -> ErrorData {
    match error.class {
        CommandErrorClass::InvalidParams => ErrorData::invalid_params(error.message, error.data),
        CommandErrorClass::InvalidRequest => ErrorData::invalid_request(error.message, error.data),
        CommandErrorClass::TemporaryUnavailable => {
            ErrorData::new(TEMPORARY_UNAVAILABLE_ERROR_CODE, error.message, error.data)
        }
        CommandErrorClass::CapacityBusy => {
            ErrorData::new(CAPACITY_BUSY_ERROR_CODE, error.message, error.data)
        }
        CommandErrorClass::Internal => ErrorData::internal_error(error.message, error.data),
    }
}

#[cfg(test)]
mod tests {
    use notegate_command::CommandError;
    use serde_json::json;

    use super::*;

    #[test]
    fn command_error_preserves_message_data_and_mcp_code() {
        let error = CommandError::temporary_unavailable("retry")
            .with_data(json!({"kind": "dependency_unavailable"}));
        let mapped = command_error(error);

        assert_eq!(mapped.code, TEMPORARY_UNAVAILABLE_ERROR_CODE);
        assert_eq!(mapped.message, "retry");
        assert_eq!(mapped.data, Some(json!({"kind": "dependency_unavailable"})));
    }
}
