//! `files_rm`: delete a path (`docs/spec/mcp/files.md`). Requires `editor`.
//!
//! Resolves the path to a node, then soft-deletes it. Folder deletion requires
//! `recursive=true`; root deletion is forbidden; an over-large subtree is
//! rejected with a narrowing hint. These rules live in the files service.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use notegate_service::files::DeleteNode;

use super::resolve::{WorkspaceSelector, caller, resolve_target, service_error};
use crate::state::AppState;

/// `files_rm` input: a workspace selector, the `path` (or `target`), and the
/// recursive flag.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the node to delete.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Required to delete a folder (and its subtree).
    #[serde(default)]
    pub recursive: bool,
}

pub async fn call(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<Input>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(
        state,
        caller,
        &input.selector,
        input.target.as_deref(),
        input.path.as_deref(),
    )
    .await?;
    let account_id = caller.account_id();
    let workspace_id = resolved.workspace_id();

    let node = state
        .files
        .resolve_path(account_id, workspace_id, &path)
        .await
        .map_err(service_error)?;

    let result = state
        .files
        .delete_node(
            account_id,
            workspace_id,
            DeleteNode {
                node_id: node.node.id,
                recursive: input.recursive,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "workspace": resolved.name(),
        "path": result.path,
        "node_id": result.node_id,
        "deleted": true,
        "purge_after": result.purge_after,
    })))
}
