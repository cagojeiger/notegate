//! `files_ls`: list the direct children of a folder (`docs/spec/mcp/files.md`).
//!
//! Resolves the folder path to a node, then keyset-paginates its live children
//! through the files service (default limit `100`, max `200`).

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use notegate_service::files::ChildrenRequest;

use super::support::page_json;
use super::resolve::{WorkspaceSelector, caller, node_summary, resolve_target, service_error};
use crate::state::AppState;

/// `files_ls` input: a workspace selector plus the folder `path` (or `target`).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute folder path inside the selected workspace.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Page size; clamped to `1..=200`, default `100`.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
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

    let folder = state
        .files
        .resolve_path(account_id, workspace_id, &path)
        .await
        .map_err(service_error)?;

    let page = state
        .files
        .children(
            account_id,
            workspace_id,
            folder.node.id,
            ChildrenRequest {
                limit: input.limit,
                cursor: input.cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let children: Vec<Value> = page.items.iter().map(node_summary).collect();
    let returned = children.len();
    let page_out = page_json(
        page.limit,
        returned,
        page.has_more,
        page.next_cursor.as_deref(),
    );

    Ok(Json(json!({
        "workspace": resolved.name(),
        "path": page.parent.path,
        "children": children,
        "page": page_out,
    })))
}
