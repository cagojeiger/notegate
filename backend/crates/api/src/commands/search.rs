//! Transport-neutral path-first search commands.

use notegate_command::{CommandError, RecoveryAction, SEARCH_OP_FIND, SEARCH_OP_GREP};
use notegate_model::NodeKind;
use notegate_search::{
    FindMatchMode, FindRequest, GrepLineMode, GrepMatchMode, GrepRequest, SearchCapacity,
    SearchDateFilters, validate_date_filters,
};
use serde_json::{Value, json};

use super::CommandContext;
use super::resolve::{actionable_input_error, resolve_target, search_error};
use super::support::page_json;
use crate::internal_search::{RequestContext, SearchClientError};
use crate::state::AppState;

#[allow(clippy::too_many_arguments)]
pub async fn find(
    state: &AppState,
    context: &CommandContext,
    target: String,
    q: String,
    kind: Option<String>,
    match_mode: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    date_filters: SearchDateFilters,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Value, CommandError> {
    let caller = context.caller();
    let search_context = request_context(context.internal_search())?;
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
            search_context,
            caller.account_id(),
            resolved.space_id(),
            FindRequest {
                date_filters,
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

    Ok(json!({
        "space": resolved.name(),
        "items": page.items,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    }))
}

#[allow(clippy::too_many_arguments)]
pub async fn grep(
    state: &AppState,
    context: &CommandContext,
    target: String,
    q: String,
    match_mode: Option<String>,
    lines: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    date_filters: SearchDateFilters,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Value, CommandError> {
    let caller = context.caller();
    let search_context = request_context(context.internal_search())?;
    let (resolved, scope_path) = resolve_target(state, caller, &target).await?;
    let scope_path = Some(scope_path);
    let space = resolved.name().to_owned();
    let match_mode = parse_grep_match_mode(match_mode.as_deref())?;
    let line_mode = parse_grep_line_mode(lines.as_deref())?;

    let page = state
        .search
        .grep(
            search_context,
            caller.account_id(),
            resolved.space_id(),
            GrepRequest {
                date_filters,
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

    Ok(json!({
        "space": space,
        "items": page.items,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    }))
}

pub(super) fn parse_date_filters(
    input: &notegate_command::SearchInput,
) -> Result<SearchDateFilters, CommandError> {
    fn parse(
        value: Option<&str>,
        field: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, CommandError> {
        value
            .map(|value| {
                chrono::DateTime::parse_from_rfc3339(value)
                    .map(|date| date.with_timezone(&chrono::Utc))
                    .map_err(|_| {
                        search_error(notegate_search::SearchError::InvalidInput(format!(
                            "{field} must be an RFC 3339 timestamp with timezone"
                        )))
                    })
            })
            .transpose()
    }
    let dates = SearchDateFilters {
        created_from: parse(input.created_from.as_deref(), "created_from")?,
        created_to: parse(input.created_to.as_deref(), "created_to")?,
        updated_from: parse(input.updated_from.as_deref(), "updated_from")?,
        updated_to: parse(input.updated_to.as_deref(), "updated_to")?,
    };
    validate_date_filters(&dates).map_err(search_error)?;
    Ok(dates)
}

fn search_client_error(error: SearchClientError) -> CommandError {
    match error {
        SearchClientError::Search(error) => search_error(error),
        SearchClientError::Capacity(capacity) => search_busy_error(capacity),
        SearchClientError::DeadlineExceeded => CommandError::temporary_unavailable(
            "search deadline exceeded; retry with a narrower target or lower limit",
        )
        .with_data(json!({
            "kind": "deadline_exceeded",
            "code": "deadline_exceeded",
            "retryable": true,
        })),
        SearchClientError::Unavailable => {
            CommandError::temporary_unavailable("search service is unavailable; retry shortly")
                .with_data(json!({
                    "kind": "search_unavailable",
                    "code": "search_unavailable",
                    "retryable": true,
                    "retry_after_ms": 1_000,
                }))
        }
    }
}

fn request_context(context: Option<&RequestContext>) -> Result<&RequestContext, CommandError> {
    context.ok_or_else(|| {
        tracing::error!(event = "internal_search.request_deadline_missing");
        search_client_error(SearchClientError::Unavailable)
    })
}

fn search_busy_error(capacity: SearchCapacity) -> CommandError {
    let operation = match capacity {
        SearchCapacity::Find => SEARCH_OP_FIND,
        SearchCapacity::Grep => SEARCH_OP_GREP,
    };
    CommandError::capacity_busy(format!("{operation} capacity is busy; retry shortly")).with_data(
        json!({
            "kind": "search_busy",
            "code": "search_busy",
            "operation": operation,
            "retryable": true,
            "retry_after_ms": 1_000,
        }),
    )
}

pub(super) fn parse_kind(value: &str) -> Result<NodeKind, CommandError> {
    NodeKind::parse(value).ok_or_else(|| {
        invalid_choice(
            "kind",
            "kind must be 'folder', 'text', or 'file'",
            &["folder", "text", "file"],
        )
    })
}

pub(super) fn parse_find_match_mode(value: Option<&str>) -> Result<FindMatchMode, CommandError> {
    FindMatchMode::parse(value.unwrap_or("contains")).ok_or_else(|| {
        invalid_choice(
            "match",
            "match must be 'contains', 'regex', or 'glob'",
            &["contains", "regex", "glob"],
        )
    })
}

pub(super) fn parse_grep_match_mode(value: Option<&str>) -> Result<GrepMatchMode, CommandError> {
    GrepMatchMode::parse(value.unwrap_or("literal")).ok_or_else(|| {
        invalid_choice(
            "match",
            "match must be 'literal' or 'regex'",
            &["literal", "regex"],
        )
    })
}

pub(super) fn parse_grep_line_mode(value: Option<&str>) -> Result<GrepLineMode, CommandError> {
    GrepLineMode::parse(value.unwrap_or("none")).ok_or_else(|| {
        invalid_choice(
            "lines",
            "lines must be 'none', 'first', or 'all'",
            &["none", "first", "all"],
        )
    })
}

fn invalid_choice(field: &'static str, message: &'static str, choices: &[&str]) -> CommandError {
    actionable_input_error(
        "invalid_field_value",
        message,
        "Choose one of the values listed by next_action.choices and retry.",
        RecoveryAction::ChooseValue {
            field: field.to_owned(),
            choices: choices.iter().map(|value| json!(value)).collect(),
        },
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use super::{
        FindMatchMode, GrepLineMode, GrepMatchMode, NodeKind, parse_find_match_mode,
        parse_grep_line_mode, parse_grep_match_mode, parse_kind, request_context,
        search_busy_error, search_client_error,
    };
    use crate::internal_search::SearchClientError;
    use notegate_command::{CommandError, CommandErrorClass};
    use notegate_search::SearchCapacity;

    fn assert_invalid_input(error: CommandError, expected_message: &str) {
        assert_eq!(error.class, CommandErrorClass::InvalidParams);
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

        assert_eq!(error.class, CommandErrorClass::CapacityBusy);
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

        assert_eq!(error.class, CommandErrorClass::TemporaryUnavailable);
        assert_eq!(
            error.message,
            "search service is unavailable; retry shortly"
        );
        let data = error.data.expect("search unavailable carries metadata");
        assert_eq!(data["kind"], "search_unavailable");
        assert_eq!(data["retryable"], true);
        assert_eq!(data["retry_after_ms"], 1_000);
    }

    #[test]
    fn search_deadline_is_distinct_from_transport_unavailability() {
        let error = search_client_error(SearchClientError::DeadlineExceeded);

        assert_eq!(error.class, CommandErrorClass::TemporaryUnavailable);
        assert_eq!(
            error.message,
            "search deadline exceeded; retry with a narrower target or lower limit"
        );
        let data = error.data.expect("deadline error carries metadata");
        assert_eq!(data["kind"], "deadline_exceeded");
        assert_eq!(data["code"], "deadline_exceeded");
        assert_eq!(data["retryable"], true);
        assert!(data.get("retry_after_ms").is_none());
    }

    #[test]
    fn missing_ingress_deadline_fails_as_search_unavailable() {
        let error = request_context(None).expect_err("deadline extension is required");

        assert_eq!(error.class, CommandErrorClass::TemporaryUnavailable);
        let data = error.data.expect("missing deadline error carries metadata");
        assert_eq!(data["code"], "search_unavailable");
        assert_eq!(data["retryable"], true);
    }
}

#[cfg(test)]
mod date_filter_tests {
    #![allow(clippy::indexing_slicing)]
    use super::parse_date_filters;
    use notegate_command::SearchInput;
    use serde_json::json;

    #[test]
    fn dates_require_timezone_and_normalize_offsets() -> Result<(), Box<dyn std::error::Error>> {
        let base = json!({"purpose":"search dates","op":"find","target":"daily:/","q":"note"});
        let input: SearchInput = serde_json::from_value(base.clone())?;
        assert_eq!(
            parse_date_filters(&input).map_err(|error| error.message)?,
            Default::default()
        );
        for field in ["created_from", "created_to", "updated_from", "updated_to"] {
            for invalid in ["2026-10-05", "2026-10-05T00:00:00", "yesterday", ""] {
                let mut value = base.clone();
                value[field] = json!(invalid);
                let input = serde_json::from_value(value)?;
                assert!(parse_date_filters(&input).is_err(), "{field}: {invalid}");
            }
        }
        let mut korean = base.clone();
        korean["updated_from"] = json!("2026-10-05T00:00:00+09:00");
        let mut utc = base;
        utc["updated_from"] = json!("2026-10-04T15:00:00Z");
        assert_eq!(
            parse_date_filters(&serde_json::from_value(korean)?).map_err(|error| error.message)?,
            parse_date_filters(&serde_json::from_value(utc)?).map_err(|error| error.message)?
        );
        Ok(())
    }
}
