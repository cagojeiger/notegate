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
    /// Page size; clamped to `1..=100`, default `50`.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// `spaces_create` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateInput {
    /// Human-friendly space name.
    pub name: String,
}

/// `spaces_get` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetInput {
    /// Human-friendly space name.
    pub name: String,
}

pub async fn list(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<ListInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
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

pub async fn get(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<GetInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let resolved = resolve_space(state, caller, &input.name).await?;
    Ok(Json(space_summary(&resolved.view)))
}
