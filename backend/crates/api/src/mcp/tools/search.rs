//! Internal search handlers used by the unified MCP `search` tool.

use axum::http::request::Parts;
use notegate_model::NodeKind;
use notegate_service::search::{
    FindMatchMode, FindRequest, GrepLineMode, GrepMatchMode, GrepRequest,
};
use rmcp::{ErrorData, Json};
use serde_json::{Value, json};

use super::resolve::{caller, invalid_input_error, node_summary, resolve_target, service_error};
use super::support::page_json;
use crate::state::AppState;

#[allow(clippy::too_many_arguments)]
pub async fn find(
    state: &AppState,
    parts: &Parts,
    target: String,
    q: String,
    kind: Option<String>,
    match_mode: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, scope_path) = resolve_target(state, caller, &target).await?;
    let scope_path = Some(scope_path);

    let kind = match kind.as_deref() {
        None => None,
        Some(value) => Some(parse_kind(value)?),
    };
    let match_mode = parse_find_match_mode(match_mode.as_deref())?;

    let page = state
        .search
        .find(
            caller.account_id(),
            resolved.space_id(),
            FindRequest {
                q,
                path: scope_path,
                kind,
                match_mode,
                include: include.unwrap_or_default(),
                exclude: exclude.unwrap_or_default(),
                limit,
                cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let items: Vec<Value> = page.items.iter().map(node_summary).collect();
    let returned = items.len();

    Ok(Json(json!({
        "space": resolved.name(),
        "items": items,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    })))
}

#[allow(clippy::too_many_arguments)]
pub async fn grep(
    state: &AppState,
    parts: &Parts,
    target: String,
    q: String,
    match_mode: Option<String>,
    lines: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, scope_path) = resolve_target(state, caller, &target).await?;
    let scope_path = Some(scope_path);
    let space = resolved.name().to_owned();
    let match_mode = parse_grep_match_mode(match_mode.as_deref())?;
    let line_mode = parse_grep_line_mode(lines.as_deref())?;

    let page = state
        .search
        .grep(
            caller.account_id(),
            resolved.space_id(),
            GrepRequest {
                q,
                path: scope_path,
                match_mode,
                line_mode,
                include: include.unwrap_or_default(),
                exclude: exclude.unwrap_or_default(),
                limit,
                cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let items: Vec<Value> = page
        .items
        .iter()
        .map(|hit| {
            let mut value = node_summary(&hit.node);
            if !hit.match_lines.is_empty()
                && let Some(object) = value.as_object_mut()
            {
                object.insert("match_lines".to_owned(), json!(hit.match_lines));
            }
            value
        })
        .collect();
    let returned = items.len();

    Ok(Json(json!({
        "space": space,
        "items": items,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    })))
}

fn parse_kind(value: &str) -> Result<NodeKind, ErrorData> {
    NodeKind::parse(value)
        .ok_or_else(|| invalid_input_error("kind must be 'folder', 'text', or 'file'"))
}

fn parse_find_match_mode(value: Option<&str>) -> Result<FindMatchMode, ErrorData> {
    match value.unwrap_or("contains") {
        "contains" => Ok(FindMatchMode::Contains),
        "regex" => Ok(FindMatchMode::Regex),
        "glob" => Ok(FindMatchMode::Glob),
        _ => Err(invalid_input_error(
            "match must be 'contains', 'regex', or 'glob'",
        )),
    }
}

fn parse_grep_match_mode(value: Option<&str>) -> Result<GrepMatchMode, ErrorData> {
    match value.unwrap_or("literal") {
        "literal" => Ok(GrepMatchMode::Literal),
        "regex" => Ok(GrepMatchMode::Regex),
        _ => Err(invalid_input_error("match must be 'literal' or 'regex'")),
    }
}

fn parse_grep_line_mode(value: Option<&str>) -> Result<GrepLineMode, ErrorData> {
    match value.unwrap_or("none") {
        "none" => Ok(GrepLineMode::None),
        "first" => Ok(GrepLineMode::First),
        "all" => Ok(GrepLineMode::All),
        _ => Err(invalid_input_error(
            "lines must be 'none', 'first', or 'all'",
        )),
    }
}
