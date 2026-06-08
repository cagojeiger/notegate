//! `files_mv`: move or rename a path within a workspace (mcp-tools.md). Requires
//! `editor`.
//!
//! Both paths live in the same workspace (cross-workspace moves are unsupported).
//! The source path resolves to the node; the destination's dirname resolves to
//! the new parent and its basename to the new name. No-op (same path), root-move,
//! sibling-conflict, and move-into-descendant rules live in the files service.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use notegate_service::files::MoveNode;

use super::resolve::{
    WorkspaceSelector, caller, node_summary, resolve_workspace, service_error, split_parent_name,
};
use crate::state::AppState;

/// `files_mv` input: a workspace selector plus source and destination paths.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the node to move.
    pub source_path: String,
    /// Absolute destination path (its dirname must be an existing folder).
    pub destination_path: String,
}

pub async fn call(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<Input>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let resolved = resolve_workspace(state, caller, &input.selector).await?;
    let account_id = caller.account_id();
    let workspace_id = resolved.workspace_id();

    if !input.source_path.starts_with('/') || !input.destination_path.starts_with('/') {
        return Err(ErrorData::invalid_params("paths must start with '/'", None));
    }

    let source = state
        .files
        .resolve_path(account_id, workspace_id, &input.source_path)
        .await
        .map_err(service_error)?;

    let (dest_parent_path, new_name) = split_parent_name(&input.destination_path)?;
    let dest_parent = state
        .files
        .resolve_path(account_id, workspace_id, &dest_parent_path)
        .await
        .map_err(service_error)?;

    let view = state
        .files
        .move_node(
            account_id,
            workspace_id,
            MoveNode {
                node_id: source.node.id,
                new_parent_node_id: dest_parent.node.id,
                new_name: Some(new_name),
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "workspace": resolved.name(),
        "node": node_summary(&view),
    })))
}
