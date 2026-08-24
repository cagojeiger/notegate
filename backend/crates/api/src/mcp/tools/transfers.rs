//! MCP adapters for transport-neutral file transfer commands.

use axum::http::request::Parts;
use notegate_command::{FileDownloadInput, FileUploadInput};
use rmcp::{ErrorData, Json};
use serde_json::Value;

use super::adapter;
use crate::commands;
use crate::state::AppState;

pub async fn upload(
    state: &AppState,
    parts: &Parts,
    input: FileUploadInput,
) -> Result<Json<Value>, ErrorData> {
    let context = adapter::context(parts)?;
    adapter::result(commands::transfers::upload(state, &context, input).await)
}

pub async fn download(
    state: &AppState,
    parts: &Parts,
    input: FileDownloadInput,
) -> Result<Json<Value>, ErrorData> {
    let context = adapter::context(parts)?;
    adapter::result(commands::transfers::download(state, &context, input).await)
}
