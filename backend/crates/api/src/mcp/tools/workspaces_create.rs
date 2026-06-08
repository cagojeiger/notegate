//! `workspaces_create`: create a workspace owned by the authenticated user caller.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::resolve::{caller, service_error, workspace_summary};
use crate::state::AppState;
use notegate_service::workspaces::CreateWorkspace;

/// `workspaces_create` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Input {
    /// Human-friendly workspace name.
    pub name: String,
}

pub async fn call(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<Input>,
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
