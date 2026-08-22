//! Internal search handlers used by the unified MCP `search` tool.

use axum::http::request::Parts;
use notegate_model::NodeKind;
use notegate_search::{
    FindMatchMode, FindRequest, GrepLineMode, GrepMatchMode, GrepRequest, SearchCapacity,
};
use rmcp::{ErrorData, Json};
use serde_json::{Value, json};

use super::resolve::{actionable_input_error, caller, resolve_target, search_error};
use super::support::page_json;
use crate::internal_search::{RequestContext, SearchClientError};
use crate::mcp::contract::{CAPACITY_BUSY_ERROR_CODE, McpAction, TEMPORARY_UNAVAILABLE_ERROR_CODE};
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
    let context = RequestContext::from_headers(&parts.headers);
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
            &context,
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
        .map_err(search_client_error)?;

    let returned = page.items.len();

    Ok(Json(json!({
        "space": resolved.name(),
        "items": page.items,
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
    let context = RequestContext::from_headers(&parts.headers);
    let (resolved, scope_path) = resolve_target(state, caller, &target).await?;
    let scope_path = Some(scope_path);
    let space = resolved.name().to_owned();
    let match_mode = parse_grep_match_mode(match_mode.as_deref())?;
    let line_mode = parse_grep_line_mode(lines.as_deref())?;

    let page = state
        .search
        .grep(
            &context,
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
        .map_err(search_client_error)?;

    let returned = page.items.len();

    Ok(Json(json!({
        "space": space,
        "items": page.items,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    })))
}

fn search_client_error(error: SearchClientError) -> ErrorData {
    match error {
        SearchClientError::Search(error) => search_error(error),
        SearchClientError::Capacity(capacity) => search_busy_error(capacity),
        SearchClientError::Unavailable => ErrorData::new(
            TEMPORARY_UNAVAILABLE_ERROR_CODE,
            "search service is unavailable; retry shortly",
            Some(json!({
                "kind": "search_unavailable",
                "code": "search_unavailable",
                "retryable": true,
                "retry_after_ms": 1_000,
            })),
        ),
    }
}

fn search_busy_error(capacity: SearchCapacity) -> ErrorData {
    let operation = match capacity {
        SearchCapacity::Find => "find",
        SearchCapacity::Grep => "grep",
    };
    ErrorData::new(
        CAPACITY_BUSY_ERROR_CODE,
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

pub(super) fn parse_kind(value: &str) -> Result<NodeKind, ErrorData> {
    NodeKind::parse(value).ok_or_else(|| {
        invalid_choice(
            "kind",
            "kind must be 'folder', 'text', or 'file'",
            &["folder", "text", "file"],
        )
    })
}

pub(super) fn parse_find_match_mode(value: Option<&str>) -> Result<FindMatchMode, ErrorData> {
    FindMatchMode::parse(value.unwrap_or("contains")).ok_or_else(|| {
        invalid_choice(
            "match",
            "match must be 'contains', 'regex', or 'glob'",
            &["contains", "regex", "glob"],
        )
    })
}

pub(super) fn parse_grep_match_mode(value: Option<&str>) -> Result<GrepMatchMode, ErrorData> {
    GrepMatchMode::parse(value.unwrap_or("literal")).ok_or_else(|| {
        invalid_choice(
            "match",
            "match must be 'literal' or 'regex'",
            &["literal", "regex"],
        )
    })
}

pub(super) fn parse_grep_line_mode(value: Option<&str>) -> Result<GrepLineMode, ErrorData> {
    GrepLineMode::parse(value.unwrap_or("none")).ok_or_else(|| {
        invalid_choice(
            "lines",
            "lines must be 'none', 'first', or 'all'",
            &["none", "first", "all"],
        )
    })
}

fn invalid_choice(field: &'static str, message: &'static str, choices: &[&str]) -> ErrorData {
    actionable_input_error(
        "invalid_field_value",
        message,
        "Choose one of the values listed by next_action.choices and retry.",
        McpAction::ChooseValue {
            field: field.to_owned(),
            choices: choices.iter().map(|value| json!(value)).collect(),
        },
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use rmcp::{ErrorData, model::ErrorCode};

    use super::{
        FindMatchMode, GrepLineMode, GrepMatchMode, NodeKind, parse_find_match_mode,
        parse_grep_line_mode, parse_grep_match_mode, parse_kind, search_busy_error,
        search_client_error,
    };
    use crate::internal_search::SearchClientError;
    use crate::mcp::contract::{CAPACITY_BUSY_ERROR_CODE, TEMPORARY_UNAVAILABLE_ERROR_CODE};
    use notegate_search::SearchCapacity;

    fn assert_invalid_input(error: ErrorData, expected_message: &str) {
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, expected_message);

        let data = error.data.expect("invalid input error carries metadata");
        assert_eq!(data["kind"], "invalid_input");
        assert_eq!(data["code"], "invalid_field_value");
        assert_eq!(data["next_action"]["kind"], "choose_value");
        assert!(
            !data["next_action"]["choices"]
                .as_array()
                .expect("choices array")
                .is_empty()
        );
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

        assert_eq!(error.code, CAPACITY_BUSY_ERROR_CODE);
        assert_eq!(error.message, "grep capacity is busy; retry shortly");
        let data = error.data.expect("search busy error carries metadata");
        assert_eq!(data["kind"], "search_busy");
        assert_eq!(data["code"], "search_busy");
        assert_eq!(data["operation"], "grep");
        assert_eq!(data["retryable"], true);
        assert_eq!(data["retry_after_ms"], 1_000);
    }

    #[test]
    fn unavailable_search_transport_is_retryable() {
        let error = search_client_error(SearchClientError::Unavailable);

        assert_eq!(error.code, TEMPORARY_UNAVAILABLE_ERROR_CODE);
        assert_eq!(
            error.message,
            "search service is unavailable; retry shortly"
        );
        let data = error.data.expect("search unavailable carries metadata");
        assert_eq!(data["kind"], "search_unavailable");
        assert_eq!(data["retryable"], true);
        assert_eq!(data["retry_after_ms"], 1_000);
    }
}
