//! `files_find`: find nodes by name under an optional scope path
//! (`docs/spec/mcp/search.md`).
//!
//! Workspace-scoped. The cursor is opaque and passed straight through. This
//! tool wires the MCP surface to the search service and maps results.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use notegate_model::NodeKind;
use notegate_service::search::FindRequest;

use super::common::page_json;
use super::resolve::{
    WorkspaceSelector, caller, invalid_input_error, node_summary, resolve_workspace, service_error,
};
use crate::state::AppState;

/// `files_find` input: a workspace selector, the query, and optional scope/kind/
/// paging fields.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// The node-name substring to match.
    pub q: String,
    /// Optional absolute scope path to search within.
    #[serde(default)]
    pub path: Option<String>,
    /// Optional kind filter: `folder` or `document`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Page size; clamped to the find limit, default `50`.
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
    let resolved = resolve_workspace(state, caller, &input.selector).await?;

    let kind = match input.kind.as_deref() {
        None => None,
        Some(value) => Some(parse_kind(value)?),
    };

    let page = state
        .search
        .find(
            caller.account_id(),
            resolved.workspace_id(),
            FindRequest {
                q: input.q,
                path: input.path,
                kind,
                limit: input.limit,
                cursor: input.cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let items: Vec<Value> = page.items.iter().map(node_summary).collect();
    let returned = items.len();
    let page_out = page_json(
        page.limit,
        returned,
        page.has_more,
        page.next_cursor.as_deref(),
    );

    Ok(Json(json!({
        "workspace": resolved.name(),
        "items": items,
        "page": page_out,
    })))
}

/// Parse a `kind` filter, rejecting unknown values.
fn parse_kind(value: &str) -> Result<NodeKind, ErrorData> {
    NodeKind::parse(value).ok_or_else(|| invalid_input_error("kind must be 'folder' or 'document'"))
}
