//! `workspaces_get`: return one workspace by name (mcp-tools.md).
//!
//! Resolves the canonical `workspace` name (or the `workspace_id` fallback)
//! against the caller's accessible workspaces. An ambiguous name yields the
//! shared ambiguity error with matching workspaces.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use serde_json::Value;

use super::resolve::{WorkspaceSelector, caller, resolve_workspace, workspace_summary};
use crate::state::AppState;

/// `workspaces_get` input: the workspace selector.
pub type Input = WorkspaceSelector;

pub async fn call(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<Input>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let resolved = resolve_workspace(state, caller, &input).await?;
    Ok(Json(workspace_summary(&resolved.view)))
}
