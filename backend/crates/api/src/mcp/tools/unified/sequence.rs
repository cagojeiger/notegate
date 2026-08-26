//! Thin MCP adapters for the shared sequence engine.

use axum::http::request::Parts;
pub use notegate_command::{RunReadSequenceInput, RunWriteSequenceInput};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use serde_json::Value;

use super::adapter;
use crate::commands::sequence;
use crate::state::AppState;

pub async fn run_read_sequence(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<RunReadSequenceInput>,
) -> Result<Json<Value>, ErrorData> {
    let context = adapter::context(parts)?;
    adapter::result(sequence::run_read(state, &context, input).await)
}

pub async fn run_write_sequence(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<RunWriteSequenceInput>,
) -> Result<Json<Value>, ErrorData> {
    let context = adapter::context(parts)?;
    adapter::result(sequence::run_write(state, &context, input).await)
}
