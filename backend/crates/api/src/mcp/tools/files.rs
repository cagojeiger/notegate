//! Test-only compatibility adapter for direct file handler assertions.

use axum::http::request::Parts;
use rmcp::{ErrorData, Json};
use serde_json::Value;

use super::adapter;
use crate::commands;
use crate::state::AppState;

pub async fn stat(
    state: &AppState,
    parts: &Parts,
    target: String,
) -> Result<Json<Value>, ErrorData> {
    let context = adapter::context(parts)?;
    adapter::result(commands::files::stat(state, &context, target).await)
}
