//! Internal search handlers used by the unified MCP `search` tool.

use axum::http::request::Parts;
use notegate_model::NodeKind;
use notegate_service::search::{
    FindMatchMode, FindRequest, GrepLineMode, GrepMatchMode, GrepRequest,
};
use rmcp::model::ErrorCode;
use rmcp::{ErrorData, Json};
use serde_json::{Value, json};

use super::resolve::{caller, invalid_input_error, node_summary, resolve_target, service_error};
use super::support::page_json;
use crate::admission::SearchCapacity;
use crate::state::AppState;

const SEARCH_BUSY_ERROR_CODE: ErrorCode = ErrorCode(-32002);

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
    let _permit = state
        .search_admission
        .enter_find()
        .map_err(search_busy_error)?;
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
    let _permit = state
        .search_admission
        .enter_grep()
        .await
        .map_err(search_busy_error)?;
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

fn search_busy_error(capacity: SearchCapacity) -> ErrorData {
    let operation = match capacity {
        SearchCapacity::Find => "find",
        SearchCapacity::Grep => "grep",
    };
    ErrorData::new(
        SEARCH_BUSY_ERROR_CODE,
        format!("{operation} capacity is busy; retry shortly"),
        Some(json!({
            "kind": "search_busy",
            "code": "search_busy",
            "operation": operation,
            "retryable": true,
            "retry_after_ms": 1_000,
        })),
    )
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use rmcp::{ErrorData, model::ErrorCode};

    use super::{
        FindMatchMode, GrepLineMode, GrepMatchMode, NodeKind, parse_find_match_mode,
        parse_grep_line_mode, parse_grep_match_mode, parse_kind, search_busy_error,
    };
    use crate::admission::SearchCapacity;

    fn assert_invalid_input(error: ErrorData, expected_message: &str) {
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, expected_message);

        let data = error.data.expect("invalid input error carries metadata");
        assert_eq!(data["kind"], "invalid_input");
        assert_eq!(data["code"], "invalid_input");
    }

    #[test]
    fn parse_kind_contract() {
        for (value, expected) in [
            ("folder", NodeKind::Folder),
            ("text", NodeKind::Text),
            ("file", NodeKind::File),
        ] {
            assert_eq!(parse_kind(value).expect("supported kind"), expected);
        }
        let error = parse_kind("document").expect_err("unknown kind must fail");
        assert_invalid_input(error, "kind must be 'folder', 'text', or 'file'");
    }

    #[test]
    fn parse_find_match_mode_contract() {
        assert_eq!(
            parse_find_match_mode(None).expect("default match mode"),
            FindMatchMode::Contains
        );
        for (value, expected) in [
            ("contains", FindMatchMode::Contains),
            ("regex", FindMatchMode::Regex),
            ("glob", FindMatchMode::Glob),
        ] {
            assert_eq!(
                parse_find_match_mode(Some(value)).expect("supported find match mode"),
                expected
            );
        }
        let error =
            parse_find_match_mode(Some("literal")).expect_err("unknown match mode must fail");
        assert_invalid_input(error, "match must be 'contains', 'regex', or 'glob'");
    }

    #[test]
    fn parse_grep_match_mode_contract() {
        assert_eq!(
            parse_grep_match_mode(None).expect("default match mode"),
            GrepMatchMode::Literal
        );
        for (value, expected) in [
            ("literal", GrepMatchMode::Literal),
            ("regex", GrepMatchMode::Regex),
        ] {
            assert_eq!(
                parse_grep_match_mode(Some(value)).expect("supported grep match mode"),
                expected
            );
        }
        let error = parse_grep_match_mode(Some("glob")).expect_err("unknown match mode must fail");
        assert_invalid_input(error, "match must be 'literal' or 'regex'");
    }

    #[test]
    fn parse_grep_line_mode_contract() {
        assert_eq!(
            parse_grep_line_mode(None).expect("default line mode"),
            GrepLineMode::None
        );
        for (value, expected) in [
            ("none", GrepLineMode::None),
            ("first", GrepLineMode::First),
            ("all", GrepLineMode::All),
        ] {
            assert_eq!(
                parse_grep_line_mode(Some(value)).expect("supported line mode"),
                expected
            );
        }
        let error =
            parse_grep_line_mode(Some("matching")).expect_err("unknown line mode must fail");
        assert_invalid_input(error, "lines must be 'none', 'first', or 'all'");
    }

    #[test]
    fn search_busy_error_is_retryable_and_names_the_operation() {
        let error = search_busy_error(SearchCapacity::Grep);

        assert_eq!(error.code, super::SEARCH_BUSY_ERROR_CODE);
        assert_eq!(error.message, "grep capacity is busy; retry shortly");
        let data = error.data.expect("search busy error carries metadata");
        assert_eq!(data["kind"], "search_busy");
        assert_eq!(data["code"], "search_busy");
        assert_eq!(data["operation"], "grep");
        assert_eq!(data["retryable"], true);
        assert_eq!(data["retry_after_ms"], 1_000);
    }
}
