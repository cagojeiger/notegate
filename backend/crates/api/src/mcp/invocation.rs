//! Shared MCP tool execution and best-effort invocation history capture.

use std::future::Future;
use std::time::Instant;

use axum::http::request::Parts;
use notegate_db::NewMcpInvocation;
use notegate_model::{Caller, CallerIdentity};
use rmcp::ErrorData;

use super::tools::resolve::{caller, invalid_input_error};
use crate::observability::observe_mcp_tool;
use crate::state::AppState;

pub(crate) const PURPOSE_MAX_CHARS: usize = 200;

pub(crate) async fn execute<T>(
    state: &AppState,
    parts: &Parts,
    tool: &'static str,
    op: Option<&str>,
    purpose: Option<&str>,
    future: impl Future<Output = Result<T, ErrorData>>,
) -> Result<T, ErrorData> {
    if let Some(purpose) = purpose
        && let Err(error) = validate_purpose(purpose)
    {
        return observe_mcp_tool(state.config.metrics_enabled, tool, async { Err(error) }).await;
    }

    let started = Instant::now();
    let result = observe_mcp_tool(state.config.metrics_enabled, tool, future).await;

    if let Ok(caller) = caller(parts) {
        record(
            state,
            caller,
            tool,
            op,
            purpose,
            &result,
            started.elapsed().as_millis(),
        )
        .await;
    }

    result
}

fn validate_purpose(purpose: &str) -> Result<(), ErrorData> {
    let char_count = purpose.chars().count();
    if char_count == 0 || purpose.trim() != purpose {
        return Err(invalid_input_error(
            "purpose must be non-empty and must not have leading or trailing whitespace",
        ));
    }
    if char_count > PURPOSE_MAX_CHARS {
        return Err(invalid_input_error(format!(
            "purpose must be at most {PURPOSE_MAX_CHARS} characters"
        )));
    }
    Ok(())
}

async fn record<T>(
    state: &AppState,
    caller: &Caller,
    tool: &'static str,
    op: Option<&str>,
    purpose: Option<&str>,
    result: &Result<T, ErrorData>,
    elapsed_ms: u128,
) {
    let (owner_user_id, caller_kind) = match &caller.identity {
        CallerIdentity::User(_) => (caller.account_id(), "user"),
        CallerIdentity::Agent(agent) => (agent.owner_user_id, "agent"),
    };
    let (outcome, error_code) = match result {
        Ok(_) => ("success", None),
        Err(error) => ("error", Some(error_code(error))),
    };
    let duration_ms = i64::try_from(elapsed_ms).unwrap_or(i64::MAX);

    if let Err(error) = state
        .mcp_invocations
        .insert(NewMcpInvocation {
            owner_user_id,
            actor_account_id: caller.account_id(),
            caller_kind,
            tool,
            op,
            purpose,
            outcome,
            error_code: error_code.as_deref(),
            duration_ms,
        })
        .await
    {
        tracing::warn!(
            tool,
            op,
            outcome,
            error = %error,
            "failed to record MCP invocation history"
        );
    }
}

fn error_code(error: &ErrorData) -> String {
    error
        .data
        .as_ref()
        .and_then(|data| data.get("code"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| error.code.0.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::unwrap_in_result
    )]

    use super::*;

    #[test]
    fn purpose_rejects_blank_padded_and_overlong_values() {
        assert!(validate_purpose("").is_err());
        assert!(validate_purpose(" search daily notes ").is_err());
        assert!(validate_purpose(&"가".repeat(PURPOSE_MAX_CHARS + 1)).is_err());
    }

    #[test]
    fn purpose_accepts_a_bounded_unicode_description() {
        assert!(validate_purpose("오늘 변경된 검색 설계 노트를 확인").is_ok());
        assert!(validate_purpose(&"가".repeat(PURPOSE_MAX_CHARS)).is_ok());
    }

    #[test]
    fn error_code_prefers_structured_application_code() {
        let error = ErrorData::invalid_params(
            "invalid input",
            Some(serde_json::json!({"code": "changes_cursor_invalid"})),
        );
        assert_eq!(error_code(&error), "changes_cursor_invalid");
    }

    #[tokio::test]
    async fn execute_records_valid_calls_without_payload() -> Result<(), Box<dyn std::error::Error>>
    {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, _space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        let mut parts = axum::http::Request::new(()).into_parts().0;
        parts.extensions.insert(caller.clone());

        execute(
            &state,
            &parts,
            "search",
            Some("find"),
            Some("locate cache design notes"),
            async { Ok::<_, ErrorData>(()) },
        )
        .await?;
        let recorded_error = execute(
            &state,
            &parts,
            "read",
            Some("read"),
            Some("read a missing design note"),
            async {
                Err::<(), _>(invalid_input_error(
                    "the requested design note does not exist",
                ))
            },
        )
        .await
        .expect_err("tool error is returned");
        assert_eq!(recorded_error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        let error = execute(&state, &parts, "read", Some("spaces"), Some(" "), async {
            Ok::<_, ErrorData>(())
        })
        .await
        .expect_err("blank purpose is rejected");
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);

        let rows = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                Option<String>,
                String,
                Option<String>,
            ),
        >(
            "SELECT tool, op, purpose, outcome, error_code \
             FROM mcp_invocations WHERE actor_account_id = $1 ORDER BY id",
        )
        .bind(caller.account_id())
        .fetch_all(&state.db)
        .await?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "search");
        assert_eq!(rows[0].1.as_deref(), Some("find"));
        assert_eq!(rows[0].2.as_deref(), Some("locate cache design notes"));
        assert_eq!(rows[0].3, "success");
        assert_eq!(rows[0].4, None);
        assert_eq!(rows[1].0, "read");
        assert_eq!(rows[1].3, "error");
        assert_eq!(rows[1].4.as_deref(), Some("invalid_input"));

        db.cleanup().await;
        Ok(())
    }
}
