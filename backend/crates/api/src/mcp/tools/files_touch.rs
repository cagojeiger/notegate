//! `files_touch`: create an empty Markdown document (mcp-tools.md). Requires
//! `editor`.
//!
//! The path's dirname resolves to the parent folder; the basename (which must
//! end in `.md`) becomes the document name. Limits and `.md` validation live in
//! the files service.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use notegate_service::files::CreateDocument;

use super::resolve::{
    WorkspaceSelector, caller, node_summary, resolve_target, service_error, split_parent_name,
};
use crate::state::AppState;

/// `files_touch` input: a workspace selector plus the document `path` (or `target`).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the `.md` document to create.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
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

    let (parent_path, name) = split_parent_name(&path)?;
    let parent = state
        .files
        .resolve_path(account_id, workspace_id, &parent_path)
        .await
        .map_err(service_error)?;

    let view = state
        .files
        .create_document(
            account_id,
            workspace_id,
            CreateDocument {
                parent_node_id: parent.node.id,
                name,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "workspace": resolved.name(),
        "node": node_summary(&view.node),
        "content_sha256": view.document.content_sha256,
        "byte_len": view.document.byte_len,
        "line_count": view.document.line_count,
    })))
}
