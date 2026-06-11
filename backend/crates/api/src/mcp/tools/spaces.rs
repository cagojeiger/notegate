//! Space MCP tools (`docs/spec/mcp/spaces.md`).

use axum::http::request::Parts;
use notegate_service::spaces::{CreateSpace, ListSpaces};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::resolve::{caller, resolve_space, service_error, space_summary};
use super::support::page_json;
use crate::state::AppState;

/// `spaces_list` input.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ListInput {
    /// Optional exact space name filter. When set, returns at most one space.
    #[serde(default)]
    pub name: Option<String>,
    /// Page size. Defaults to 50 and is clamped by the service.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque cursor from the previous `spaces_list` page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// `spaces_create` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateInput {
    /// Human-friendly unique space name for path targets such as `<name>:/`.
    pub name: String,
}

pub async fn list(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<ListInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    if let Some(name) = input.name {
        let resolved = resolve_space(state, caller, &name).await?;
        return Ok(Json(json!({
            "spaces": [space_summary(&resolved.view)],
            "page": page_json(1, 1, false, None),
        })));
    }

    let page = state
        .spaces
        .list(
            caller.account_id(),
            ListSpaces {
                limit: input.limit,
                cursor: input.cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let spaces: Vec<Value> = page.items.iter().map(space_summary).collect();
    let returned = spaces.len();

    Ok(Json(json!({
        "spaces": spaces,
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
        .spaces
        .create(
            caller.account.kind,
            caller.account_id(),
            CreateSpace { name: input.name },
        )
        .await
        .map_err(service_error)?;
    Ok(Json(space_summary(&view)))
}
