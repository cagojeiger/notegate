//! Workspace MCP tools (`docs/spec/mcp/workspaces.md`).

use axum::http::request::Parts;
use notegate_service::workspaces::{CreateWorkspace, ListWorkspaces};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::resolve::{
    WorkspaceSelector, caller, resolve_workspace, service_error, workspace_summary,
};
use super::support::page_json;
use crate::state::AppState;

/// `workspaces_list` input.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ListInput {
    /// Page size; clamped to `1..=100`, default `50`.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// `workspaces_create` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateInput {
    /// Human-friendly workspace name.
    pub name: String,
}

/// `workspaces_get` input: the workspace selector.
pub type GetInput = WorkspaceSelector;

pub async fn list(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<ListInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let page = state
        .workspaces
        .list(
            caller.account_id(),
            ListWorkspaces {
                limit: input.limit,
                cursor: input.cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let workspaces: Vec<Value> = page.items.iter().map(workspace_summary).collect();
    let returned = workspaces.len();

    Ok(Json(json!({
        "workspaces": workspaces,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    })))
}

pub async fn create(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<CreateInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let view = state
        .workspaces
        .create(
            caller.account.kind,
            caller.account_id(),
            CreateWorkspace { name: input.name },
        )
        .await
        .map_err(service_error)?;
    Ok(Json(workspace_summary(&view)))
}

pub async fn get(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<GetInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let resolved = resolve_workspace(state, caller, &input).await?;
    Ok(Json(workspace_summary(&resolved.view)))
}
