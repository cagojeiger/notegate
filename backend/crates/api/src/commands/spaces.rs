//! Transport-neutral space listing command.

use notegate_command::CommandError;
use notegate_service::spaces::ListSpaces;
use serde_json::{Value, json};

use super::CommandContext;
use super::resolve::{resolve_space, service_error, space_summary};
use super::support::page_json;
use crate::state::AppState;

pub async fn list(
    state: &AppState,
    context: &CommandContext,
    name: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Value, CommandError> {
    if let Some(name) = name {
        let resolved = resolve_space(state, context.caller(), &name).await?;
        return Ok(json!({
            "spaces": [space_summary(&resolved.view)],
            "page": page_json(1, 1, false, None),
        }));
    }

    let page = state
        .spaces
        .list_mcp(context.caller().account_id(), ListSpaces { limit, cursor })
        .await
        .map_err(service_error)?;

    let spaces: Vec<Value> = page.items.iter().map(space_summary).collect();
    let returned = spaces.len();

    Ok(json!({
        "spaces": spaces,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    }))
}
