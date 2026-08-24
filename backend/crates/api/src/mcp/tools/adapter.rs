//! Thin MCP boundary around the transport-neutral command executor.

use axum::http::request::Parts;
use notegate_command::CommandError;
use notegate_model::Caller;
use rmcp::{ErrorData, Json};
use serde_json::Value;

use crate::commands::CommandContext;
use crate::internal_search::RequestContext;
use crate::mcp::contract::command_error;

pub(super) fn context(parts: &Parts) -> Result<CommandContext, ErrorData> {
    let caller = parts.extensions.get::<Caller>().cloned().ok_or_else(|| {
        command_error(crate::commands::error::invalid_input_error(
            "authenticated caller extension missing",
        ))
    })?;
    Ok(CommandContext::new(
        caller,
        RequestContext::from_parts(parts),
    ))
}

pub(super) fn result(result: Result<Value, CommandError>) -> Result<Json<Value>, ErrorData> {
    result.map(Json).map_err(command_error)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use axum::http::Request;

    use super::*;

    #[test]
    fn missing_caller_keeps_the_existing_structured_invalid_input_error() {
        let parts = Request::new(()).into_parts().0;
        let error = context(&parts).expect_err("caller is required");

        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert_eq!(
            error.data.as_ref().and_then(|data| data["kind"].as_str()),
            Some("invalid_input")
        );
    }
}
