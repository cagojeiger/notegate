//! Internal space listing handler used by the unified MCP `read` tool.

use axum::http::request::Parts;
use notegate_service::spaces::ListSpaces;
use rmcp::{ErrorData, Json};
use serde_json::{Value, json};

use super::resolve::{caller, resolve_space, service_error, space_summary};
use super::support::page_json;
use crate::state::AppState;

pub async fn list(
    state: &AppState,
    parts: &Parts,
    name: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    if let Some(name) = name {
        let resolved = resolve_space(state, caller, &name).await?;
        return Ok(Json(json!({
            "spaces": [space_summary(&resolved.view)],
            "page": page_json(1, 1, false, None),
        })));
    }

    let page = state
        .spaces
        .list(caller.account_id(), ListSpaces { limit, cursor })
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
