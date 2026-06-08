//! `files_find`: find nodes by name under an optional scope path
//! (mcp-tools.md).
//!
//! Workspace-scoped. The cursor is opaque and passed straight through. This
//! tool wires the MCP surface to the search service and maps results.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use notegate_core::limits;
use notegate_model::NodeKind;
use notegate_service::search::FindRequest;

use super::resolve::{
    WorkspaceSelector, caller, encode_cursor, node_summary, resolve_workspace, service_error,
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
    let limit = clamp(
        input.limit,
        limits::FIND_DEFAULT_LIMIT,
        limits::FIND_MAX_LIMIT,
    );

    let page = state
        .search
        .find(
            caller.account_id(),
            resolved.workspace_id(),
            FindRequest {
                q: input.q,
                path: input.path,
                kind,
                limit: Some(limit),
                cursor: input.cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let next_cursor = match page.next_cursor.as_ref() {
        Some(cursor) => Some(encode_cursor(cursor)?),
        None => None,
    };

    let items: Vec<Value> = page.items.iter().map(node_summary).collect();
    let returned = items.len();

    Ok(Json(json!({
        "workspace": resolved.name(),
        "items": items,
        "page": {
            "limit": page.limit,
            "returned": returned,
            "has_more": page.has_more,
            "next_cursor": next_cursor,
        },
    })))
}

/// Parse a `kind` filter, rejecting unknown values.
fn parse_kind(value: &str) -> Result<NodeKind, ErrorData> {
    NodeKind::parse(value)
        .ok_or_else(|| ErrorData::invalid_params("kind must be 'folder' or 'document'", None))
}

/// Clamp a requested limit to `1..=max`, defaulting to `default`.
fn clamp(limit: Option<i64>, default: i64, max: i64) -> i64 {
    match limit {
        None => default,
        Some(value) if value < 1 => 1,
        Some(value) => value.min(max),
    }
}
