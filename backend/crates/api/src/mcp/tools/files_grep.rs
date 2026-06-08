//! `files_grep`: search Markdown body lines under an optional scope path
//! (`docs/spec/mcp/search.md`).
//!
//! Workspace-scoped. The cursor is opaque and passed straight through. This tool
//! wires the MCP surface to the search service and maps match items.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use notegate_service::search::GrepRequest;

use super::support::page_json;
use super::resolve::{WorkspaceSelector, caller, resolve_target, resolve_workspace, service_error};
use crate::state::AppState;

/// `files_grep` input: a workspace selector, the query, and optional target/
/// scope/context/paging fields.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// The line substring to match.
    pub q: String,
    /// Optional absolute scope path to search within.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<scope-path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Lines of surrounding context to include per match.
    #[serde(default)]
    pub context: Option<i64>,
    /// Page size; clamped to the grep limit, default `20`.
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
    let (resolved, scope_path) = match input.target.as_deref() {
        Some(target) => {
            let (resolved, path) =
                resolve_target(state, caller, &input.selector, Some(target), None).await?;
            (resolved, Some(path))
        }
        None => (
            resolve_workspace(state, caller, &input.selector).await?,
            input.path,
        ),
    };
    let workspace = resolved.name().to_owned();

    let page = state
        .search
        .grep(
            caller.account_id(),
            resolved.workspace_id(),
            GrepRequest {
                q: input.q,
                path: scope_path,
                context: input.context,
                limit: input.limit,
                cursor: input.cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let matches: Vec<Value> = page
        .items
        .iter()
        .map(|m| {
            json!({
                "workspace": workspace,
                "path": m.path,
                "line_no": m.line_no,
                "line": m.line,
                "before": m.before,
                "after": m.after,
            })
        })
        .collect();
    let returned = matches.len();
    let page_out = page_json(
        page.limit,
        returned,
        page.has_more,
        page.next_cursor.as_deref(),
    );

    Ok(Json(json!({
        "workspace": workspace,
        "matches": matches,
        "page": page_out,
    })))
}
