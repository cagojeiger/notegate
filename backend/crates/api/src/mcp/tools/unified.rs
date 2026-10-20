//! Unified MCP adapters for transport-neutral NoteGate commands and sequences.

use axum::http::request::Parts;
#[cfg(test)]
pub use notegate_command::CompletedPartInput;
pub use notegate_command::{
    FileDownloadInput, FileUploadInput, ManageInput, ReadInput, SearchInput, WriteInput,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use serde_json::Value;

use super::adapter;
use crate::commands;
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
    adapter::result(commands::executor::read(state, &adapter::context(parts)?, input).await)
}

pub async fn search(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<SearchInput>,
) -> Result<Json<Value>, ErrorData> {
    adapter::result(commands::executor::search(state, &adapter::context(parts)?, input).await)
}

pub async fn write(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<WriteInput>,
) -> Result<Json<Value>, ErrorData> {
    adapter::result(commands::executor::write(state, &adapter::context(parts)?, input).await)
}

pub async fn manage(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<ManageInput>,
) -> Result<Json<Value>, ErrorData> {
    adapter::result(commands::executor::manage(state, &adapter::context(parts)?, input).await)
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
