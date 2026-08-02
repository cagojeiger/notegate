//! Shared MCP tool execution and best-effort invocation history capture.

use std::future::Future;
use std::time::Instant;

use axum::http::request::Parts;
use notegate_db::NewMcpInvocation;
use notegate_model::{Caller, CallerIdentity};
use rmcp::{ErrorData, Json};
use serde_json::Value;

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
    execute_classified(state, parts, tool, op, purpose, future, |_| None).await
}

pub(crate) async fn execute_sequence(
    state: &AppState,
    parts: &Parts,
    purpose: &str,
    future: impl Future<Output = Result<Json<Value>, ErrorData>>,
) -> Result<Json<Value>, ErrorData> {
    execute_classified(
        state,
        parts,
        "run_sequence",
        None,
        Some(purpose),
        future,
        sequence_error_code,
    )
    .await
}

async fn execute_classified<T>(
    state: &AppState,
    parts: &Parts,
    tool: &'static str,
    op: Option<&str>,
    purpose: Option<&str>,
    future: impl Future<Output = Result<T, ErrorData>>,
    classify_error: impl FnOnce(&T) -> Option<String>,
) -> Result<T, ErrorData> {
    if let Some(purpose) = purpose
        && let Err(error) = validate_purpose(purpose)
    {
        return observe_mcp_tool(state.config.metrics_enabled, tool, async { Err(error) }).await;
    }

    let started = Instant::now();
    let result = observe_mcp_tool(state.config.metrics_enabled, tool, future).await;
    let classified_error = result.as_ref().ok().and_then(classify_error);

    if let Ok(caller) = caller(parts) {
        record(
            state,
            caller,
            InvocationRecord {
                tool,
                op,
                purpose,
                classified_error: classified_error.as_deref(),
                elapsed_ms: started.elapsed().as_millis(),
            },
            &result,
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

struct InvocationRecord<'a> {
    tool: &'static str,
    op: Option<&'a str>,
    purpose: Option<&'a str>,
    classified_error: Option<&'a str>,
    elapsed_ms: u128,
}

async fn record<T>(
    state: &AppState,
    caller: &Caller,
    invocation: InvocationRecord<'_>,
    result: &Result<T, ErrorData>,
) {
    let (owner_user_id, caller_kind) = match &caller.identity {
        CallerIdentity::User(_) => (caller.account_id(), "user"),
        CallerIdentity::Agent(agent) => (agent.owner_user_id, "agent"),
    };
    let error_code = match result {
        Ok(_) => invocation.classified_error.map(str::to_owned),
        Err(error) => Some(error_code(error)),
    };
    let outcome = if error_code.is_some() {
        "error"
    } else {
        "success"
    };
    let duration_ms = i64::try_from(invocation.elapsed_ms).unwrap_or(i64::MAX);

    if let Err(error) = state
        .mcp_invocations
        .insert(NewMcpInvocation {
            owner_user_id,
            actor_account_id: caller.account_id(),
            caller_kind,
            tool: invocation.tool,
            op: invocation.op,
            purpose: invocation.purpose,
            outcome,
            error_code: error_code.as_deref(),
            duration_ms,
        })
        .await
    {
        tracing::warn!(
            tool = invocation.tool,
            op = invocation.op,
            outcome,
            error = %error,
            "failed to record MCP invocation history"
        );
    }
}

fn sequence_error_code(result: &Json<Value>) -> Option<String> {
    if result.0.get("ok").and_then(Value::as_bool) != Some(false) {
        return None;
    }

    result
        .0
        .pointer("/error/data/code")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            result
                .0
                .pointer("/error/code")
                .and_then(Value::as_i64)
                .map(|code| code.to_string())
        })
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

    #[test]
    fn sequence_error_code_reads_failed_sequence_results() {
        let application_error = Json(serde_json::json!({
            "ok": false,
            "error": {
                "code": -32602,
                "data": {"code": "invalid_input"}
            }
        }));
        assert_eq!(
            sequence_error_code(&application_error).as_deref(),
            Some("invalid_input")
        );

        let success = Json(serde_json::json!({"ok": true}));
        assert_eq!(sequence_error_code(&success), None);
    }

    #[tokio::test]
    async fn execute_records_tool_results_without_payload() -> Result<(), Box<dyn std::error::Error>>
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
        let sequence = execute_sequence(&state, &parts, "run a failing sequence", async {
            Ok(Json(serde_json::json!({
                "ok": false,
                "completed": 0,
                "failed_index": 0,
                "results": [],
                "error": {
                    "code": -32602,
                    "message": "invalid input",
                    "data": {"code": "invalid_input"}
                }
            })))
        })
        .await?;
        assert_eq!(sequence.0["ok"], false);

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
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, "search");
        assert_eq!(rows[0].1.as_deref(), Some("find"));
        assert_eq!(rows[0].2.as_deref(), Some("locate cache design notes"));
        assert_eq!(rows[0].3, "success");
        assert_eq!(rows[0].4, None);
        assert_eq!(rows[1].0, "read");
        assert_eq!(rows[1].3, "error");
        assert_eq!(rows[1].4.as_deref(), Some("invalid_input"));
        assert_eq!(rows[2].0, "run_sequence");
        assert_eq!(rows[2].2.as_deref(), Some("run a failing sequence"));
        assert_eq!(rows[2].3, "error");
        assert_eq!(rows[2].4.as_deref(), Some("invalid_input"));

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn execute_records_agent_owner_and_actor() -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, _space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        let owner_user_id = caller.account_id();
        let agent_account_id = uuid::Uuid::new_v4();
        let mut agent_account = caller.account;
        agent_account.id = agent_account_id;
        agent_account.kind = notegate_model::account::AccountKind::Agent;
        agent_account.display_name = "Search Agent".to_owned();
        let agent_caller = Caller {
            account: agent_account,
            identity: CallerIdentity::Agent(notegate_model::agent::Agent {
                id: agent_account_id,
                name: "Search Agent".to_owned(),
                owner_user_id,
            }),
            channel: notegate_model::Channel::Mcp,
        };
        let mut parts = axum::http::Request::new(()).into_parts().0;
        parts.extensions.insert(agent_caller);

        execute(
            &state,
            &parts,
            "search",
            Some("grep"),
            Some("search the owner's notes"),
            async { Ok::<_, ErrorData>(()) },
        )
        .await?;

        let row = sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid, String)>(
            "SELECT owner_user_id, actor_account_id, caller_kind \
             FROM mcp_invocations WHERE purpose = $1",
        )
        .bind("search the owner's notes")
        .fetch_one(&state.db)
        .await?;
        assert_eq!(row.0, owner_user_id);
        assert_eq!(row.1, agent_account_id);
        assert_eq!(row.2, "agent");

        db.cleanup().await;
        Ok(())
    }
}
