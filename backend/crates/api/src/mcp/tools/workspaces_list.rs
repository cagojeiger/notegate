//! `workspaces_list`: list workspaces accessible to the caller (`docs/spec/mcp/workspaces.md`).
//!
//! Use before file tools when the caller has more than one workspace. Default
//! limit `50`, max `100`. Pagination is delegated to the workspace service so
//! DB access stays bounded.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::common::page_json;
use super::resolve::{caller, service_error, workspace_summary};
use crate::state::AppState;
use notegate_service::workspaces::ListWorkspaces;

/// `workspaces_list` input.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    /// Page size; clamped to `1..=100`, default `50`.
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
