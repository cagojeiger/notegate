//! Unified MCP adapters for transport-neutral NoteGate commands and sequences.

use axum::http::request::Parts;
#[cfg(test)]
pub use notegate_command::CompletedPartInput;
pub use notegate_command::{
    FileDownloadInput, FileUploadInput, ManageInput, ReadInput, SearchInput, WriteInput,
};
use notegate_command::{
    ManageOperationSchema, ReadOperationSchema, SearchOperationSchema, WriteEditEntrySchema,
    WriteOperationSchema,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use serde_json::Value;

use super::adapter;
use crate::commands::{self, CommandContext};
use crate::mcp::contract::{McpAction, command_error};
use crate::state::AppState;

mod sequence;

pub use sequence::{
    RunReadSequenceInput, RunWriteSequenceInput, run_read_sequence, run_write_sequence,
};

pub async fn read(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<ReadInput>,
) -> Result<Json<Value>, ErrorData> {
    execute_read(state, &adapter::context(parts)?, input).await
}

async fn execute_read(
    state: &AppState,
    context: &CommandContext,
    input: ReadInput,
) -> Result<Json<Value>, ErrorData> {
    adapter::result(commands::executor::read(state, context, input).await)
}

pub async fn search(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<SearchInput>,
) -> Result<Json<Value>, ErrorData> {
    execute_search(state, &adapter::context(parts)?, input).await
}

async fn execute_search(
    state: &AppState,
    context: &CommandContext,
    input: SearchInput,
) -> Result<Json<Value>, ErrorData> {
    adapter::result(commands::executor::search(state, context, input).await)
}

pub async fn write(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<WriteInput>,
) -> Result<Json<Value>, ErrorData> {
    execute_write(state, &adapter::context(parts)?, input).await
}

async fn execute_write(
    state: &AppState,
    context: &CommandContext,
    input: WriteInput,
) -> Result<Json<Value>, ErrorData> {
    adapter::result(commands::executor::write(state, context, input).await)
}

pub async fn manage(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<ManageInput>,
) -> Result<Json<Value>, ErrorData> {
    execute_manage(state, &adapter::context(parts)?, input).await
}

async fn execute_manage(
    state: &AppState,
    context: &CommandContext,
    input: ManageInput,
) -> Result<Json<Value>, ErrorData> {
    adapter::result(commands::executor::manage(state, context, input).await)
}

fn validate_read_operation(input: &ReadInput) -> Result<(), ErrorData> {
    commands::executor::validate_read_operation(input).map_err(command_error)
}

fn validate_search_operation(input: &SearchInput) -> Result<(), ErrorData> {
    commands::executor::validate_search_operation(input).map_err(command_error)
}

fn validate_write_operation(input: &WriteInput) -> Result<(), ErrorData> {
    commands::executor::validate_write_operation(input).map_err(command_error)
}

fn validate_static_write_content(input: &WriteInput) -> Result<(), ErrorData> {
    commands::executor::validate_static_write_content(input).map_err(command_error)
}

fn validate_manage_operation(input: &ManageInput) -> Result<(), ErrorData> {
    commands::executor::validate_manage_operation(input).map_err(command_error)
}

fn required<T>(
    value: Option<T>,
    field: &'static str,
    context: &'static str,
) -> Result<T, ErrorData> {
    commands::error::required_input(value, field, context).map_err(command_error)
}

fn invalid_input_error(message: impl Into<String>) -> ErrorData {
    command_error(commands::error::invalid_input_error(message))
}

fn actionable_input_error(
    code: &'static str,
    message: impl Into<String>,
    hint: &'static str,
    next_action: McpAction,
) -> ErrorData {
    command_error(commands::error::actionable_input_error(
        code,
        message,
        hint,
        next_action,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{FileUploadInput, ManageInput, WriteInput};

    #[test]
    fn mutation_tools_reject_node_metadata_fields() {
        assert!(
            serde_json::from_value::<WriteInput>(json!({
                "purpose": "verify metadata boundary",
                "op": "write",
                "target": "daily:/note.md",
                "metadata": {}
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ManageInput>(json!({
                "purpose": "verify metadata boundary",
                "op": "mkdir",
                "target": "daily:/folder",
                "metadata": {}
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<FileUploadInput>(json!({
                "purpose": "verify metadata boundary",
                "op": "complete_upload",
                "upload_id": "upload-id",
                "node_metadata": {}
            }))
            .is_err()
        );
    }
}
