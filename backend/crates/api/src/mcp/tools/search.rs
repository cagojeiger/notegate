//! Compatibility MCP adapters for direct search handler tests.

use axum::http::request::Parts;
use rmcp::{ErrorData, Json};
use serde_json::Value;

use super::adapter;
use crate::commands;
use crate::state::AppState;

#[allow(clippy::too_many_arguments)]
pub async fn find(
    state: &AppState,
    parts: &Parts,
    target: String,
    q: String,
    kind: Option<String>,
    match_mode: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let context = adapter::context(parts)?;
    adapter::result(
        commands::search::find(
            state, &context, target, q, kind, match_mode, include, exclude, limit, cursor,
        )
        .await,
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn grep(
    state: &AppState,
    parts: &Parts,
    target: String,
    q: String,
    match_mode: Option<String>,
    lines: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let context = adapter::context(parts)?;
    adapter::result(
        commands::search::grep(
            state, &context, target, q, match_mode, lines, include, exclude, limit, cursor,
        )
        .await,
    )
}
