//! `files_restore`: restore a soft-deleted node. Requires `editor`.
//!
//! A deleted path no longer resolves to a live node, so restore is addressed by
//! `node_id` (the one tool where the node-id workflow is unavoidable). The files
//! service re-validates sibling-name uniqueness, fanout, and depth, and rejects
//! the restore when an ancestor is still deleted (with a hint to restore the
//! ancestor first) — the locked orphan rule.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use notegate_service::files::RestoreNode;

use super::resolve::{WorkspaceSelector, caller, node_summary, resolve_workspace, service_error};
use crate::state::AppState;

/// `files_restore` input: a workspace selector plus the deleted node's id.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// The id (UUID string) of the soft-deleted node to restore.
    pub node_id: String,
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

    let node_id = Uuid::parse_str(&input.node_id)
        .map_err(|_error| ErrorData::invalid_params("node_id must be a UUID", None))?;

    let view = state
        .files
        .restore_node(account_id, workspace_id, RestoreNode { node_id })
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "workspace": resolved.name(),
        "node": node_summary(&view),
    })))
}
