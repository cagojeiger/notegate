//! Shared MCP tool execution and best-effort invocation history capture.

use std::future::Future;
use std::time::Instant;

use notegate_db::NewMcpInvocation;
use notegate_model::{Caller, CallerIdentity};
use notegate_service::files::parse_target;
use rmcp::ErrorData;
use rmcp::model::{CallToolResponse, CallToolResult};
use serde_json::Value;

use super::invocation_redaction::{redact_input, redact_response};
use super::tools::resolve::invalid_input_error;
use crate::observability::record_mcp_tool_metrics;
use crate::state::AppState;

pub(crate) const PURPOSE_MAX_CHARS: usize = 200;

pub(crate) async fn execute_call(
    state: &AppState,
    caller: Option<&Caller>,
    tool: &str,
    input: &Value,
    future: impl Future<Output = Result<CallToolResponse, ErrorData>>,
) -> Result<CallToolResponse, ErrorData> {
    let raw_op = input.get("op").and_then(Value::as_str);
    let op = canonical_op(tool, raw_op);
    let raw_purpose = if tool == "me" {
        None
    } else {
        input.get("purpose").and_then(Value::as_str)
    };
    let purpose_validation = raw_purpose.map(validate_purpose).transpose();
    let purpose = raw_purpose.filter(|_| purpose_validation.is_ok());
    let space_name = if tool == "read" && op == Some("changes") {
        invocation_space_name(input.get("target").and_then(Value::as_str))
    } else {
        None
    };

    let started = Instant::now();
    let result = match purpose_validation {
        Ok(_) => future.await,
        Err(error) => Err(error),
    };
    let error_code = call_error_code(tool, &result);
    let outcome = if error_code.is_some() {
        "error"
    } else {
        "success"
    };
    let elapsed = started.elapsed();
    record_mcp_tool_metrics(
        state.config.metrics_enabled,
        metric_tool_name(tool),
        outcome,
        elapsed,
    );

    if let Some(caller) = caller {
        let redacted_input = redact_input(tool, input);
        let redacted_response = redact_response(tool, input, &result);
        record(
            state,
            caller,
            InvocationRecord {
                tool: canonical_tool(tool),
                op,
                purpose,
                space_name: space_name.as_deref(),
                input: &redacted_input,
                response: &redacted_response,
                error_code: error_code.as_deref(),
                elapsed_ms: elapsed.as_millis(),
            },
        )
        .await;
    }

    result
}

/// Extract the validated Space-name segment used by the invocation list summary.
pub(crate) fn invocation_space_name(target: Option<&str>) -> Option<String> {
    target
        .and_then(|target| parse_target(target).ok())
        .map(|target| target.space)
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
    tool: &'a str,
    op: Option<&'a str>,
    purpose: Option<&'a str>,
    space_name: Option<&'a str>,
    input: &'a Value,
    response: &'a Value,
    error_code: Option<&'a str>,
    elapsed_ms: u128,
}

async fn record(state: &AppState, caller: &Caller, invocation: InvocationRecord<'_>) {
    let (owner_user_id, caller_kind) = match &caller.identity {
        CallerIdentity::User(_) => (caller.account_id(), "user"),
        CallerIdentity::Agent(agent) => (agent.owner_user_id, "agent"),
    };
    let outcome = if invocation.error_code.is_some() {
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
            space_name: invocation.space_name,
            input: invocation.input,
            response: Some(invocation.response),
            outcome,
            error_code: invocation.error_code,
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

fn call_error_code(tool: &str, result: &Result<CallToolResponse, ErrorData>) -> Option<String> {
    match result {
        Err(error) => Some(error_code(error)),
        Ok(CallToolResponse::Complete(result)) if result.is_error == Some(true) => {
            Some(tool_result_error_code(result))
        }
        Ok(CallToolResponse::Complete(result)) if tool == "run_sequence" => result
            .structured_content
            .as_ref()
            .and_then(sequence_error_code),
        Ok(_) => None,
    }
}

fn tool_result_error_code(result: &CallToolResult) -> String {
    let argument_error = result
        .content
        .first()
        .and_then(|content| content.as_text())
        .is_some_and(|text| text.text.starts_with("failed to deserialize parameters:"));
    if argument_error {
        "invalid_params".to_owned()
    } else {
        "tool_error".to_owned()
    }
}

fn sequence_error_code(result: &Value) -> Option<String> {
    if result.get("ok").and_then(Value::as_bool) != Some(false) {
        return None;
    }

    result
        .pointer("/error/data/code")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            result
                .pointer("/error/code")
                .and_then(Value::as_i64)
                .map(|code| code.to_string())
        })
}

fn metric_tool_name(tool: &str) -> &'static str {
    match tool {
        "me" => "me",
        "read" => "read",
        "search" => "search",
        "write" => "write",
        "manage" => "manage",
        "file_transfer" => "file_transfer",
        "run_sequence" => "run_sequence",
        _ => "unknown",
    }
}

fn canonical_tool(tool: &str) -> &'static str {
    metric_tool_name(tool)
}

fn canonical_op<'a>(tool: &str, op: Option<&'a str>) -> Option<&'a str> {
    match (tool, op?) {
        ("read", op @ ("spaces" | "ls" | "tree" | "stat" | "read" | "changes"))
        | ("search", op @ ("find" | "grep"))
        | ("write", op @ ("write" | "append" | "patch" | "edit"))
        | ("manage", op @ ("mkdir" | "mv" | "cp" | "rm"))
        | (
            "file_transfer",
            op @ ("begin_upload" | "prepare_parts" | "complete_upload" | "abort_upload"
            | "prepare_download"),
        ) => Some(op),
        _ => None,
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
        clippy::panic,
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
    fn invocation_space_name_keeps_only_a_valid_space_segment() {
        assert_eq!(
            invocation_space_name(Some("Daily Research:/private/note.md")).as_deref(),
            Some("Daily Research")
        );
        assert_eq!(invocation_space_name(Some("missing-colon")), None);
        assert_eq!(invocation_space_name(None), None);
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
        let application_error = serde_json::json!({
            "ok": false,
            "error": {
                "code": -32602,
                "data": {"code": "invalid_input"}
            }
        });
        assert_eq!(
            sequence_error_code(&application_error).as_deref(),
            Some("invalid_input")
        );

        let success = serde_json::json!({"ok": true});
        assert_eq!(sequence_error_code(&success), None);
    }

    #[tokio::test]
    async fn execute_call_records_redacted_inputs_responses_and_all_outcomes()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, _space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        let search_input = serde_json::json!({
            "purpose": "locate cache design notes",
            "op": "find",
            "target": "Research:/",
            "q": "SECRET_SEARCH_QUERY"
        });
        execute_call(&state, Some(&caller), "search", &search_input, async {
            Ok(CallToolResult::structured(serde_json::json!({"items": []})).into())
        })
        .await?;
        let missing_input = serde_json::json!({
            "purpose": "read a missing design note",
            "op": "read",
            "target": "Research:/missing.md"
        });
        let recorded_error = execute_call(&state, Some(&caller), "read", &missing_input, async {
            Err::<CallToolResponse, _>(invalid_input_error(
                "the requested design note does not exist",
            ))
        })
        .await
        .expect_err("tool error is returned");
        assert_eq!(recorded_error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        let invalid_purpose_input = serde_json::json!({"purpose": " ", "op": "spaces"});
        let error = execute_call(
            &state,
            Some(&caller),
            "read",
            &invalid_purpose_input,
            async { Ok(CallToolResult::structured(serde_json::json!({"spaces": []})).into()) },
        )
        .await
        .expect_err("blank purpose is rejected");
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        let sequence_input = serde_json::json!({
            "purpose": "run a failing sequence",
            "commands": [{"tool": "read", "op": "read", "target": "Research:/missing.md"}]
        });
        let sequence = execute_call(
            &state,
            Some(&caller),
            "run_sequence",
            &sequence_input,
            async {
                Ok(CallToolResult::structured(serde_json::json!({
                    "ok": false,
                    "completed": 0,
                    "failed_index": 0,
                    "results": [],
                    "error": {
                        "code": -32602,
                        "message": "invalid input",
                        "data": {"code": "invalid_input"}
                    }
                }))
                .into())
            },
        )
        .await?;
        let CallToolResponse::Complete(sequence) = sequence else {
            panic!("expected a complete sequence response");
        };
        assert_eq!(
            sequence
                .structured_content
                .as_ref()
                .expect("sequence structured content")["ok"],
            false
        );

        let rows = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                Option<String>,
                serde_json::Value,
                serde_json::Value,
                String,
                Option<String>,
            ),
        >(
            "SELECT tool, op, purpose, input, response, outcome, error_code \
             FROM mcp_invocations WHERE actor_account_id = $1 ORDER BY id",
        )
        .bind(caller.account_id())
        .fetch_all(&state.db)
        .await?;
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].0, "search");
        assert_eq!(rows[0].1.as_deref(), Some("find"));
        assert_eq!(rows[0].2.as_deref(), Some("locate cache design notes"));
        assert_eq!(rows[0].3["q"]["category"], "search_query");
        assert_eq!(rows[0].4["result"]["items"], serde_json::json!([]));
        assert_eq!(rows[0].5, "success");
        assert_eq!(rows[0].6, None);
        assert!(!rows[0].3.to_string().contains("SECRET_SEARCH_QUERY"));
        assert_eq!(rows[1].0, "read");
        assert_eq!(rows[1].3, missing_input);
        assert_eq!(rows[1].4["kind"], "error");
        assert_eq!(rows[1].5, "error");
        assert_eq!(rows[1].6.as_deref(), Some("invalid_input"));
        assert_eq!(rows[2].0, "read");
        assert_eq!(rows[2].2, None);
        assert_eq!(rows[2].3["purpose"]["category"], "invalid_purpose");
        assert_eq!(rows[2].4["kind"], "error");
        assert_eq!(rows[2].5, "error");
        assert_eq!(rows[2].6.as_deref(), Some("invalid_input"));
        assert_eq!(rows[3].0, "run_sequence");
        assert_eq!(rows[3].2.as_deref(), Some("run a failing sequence"));
        assert_eq!(rows[3].3, sequence_input);
        assert_eq!(rows[3].4["result"]["ok"], false);
        assert_eq!(rows[3].5, "error");
        assert_eq!(rows[3].6.as_deref(), Some("invalid_input"));

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn execute_call_persists_redacted_high_risk_payloads()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, _space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;

        let upload_input = serde_json::json!({
            "purpose": "prepare upload for a report",
            "op": "begin_upload",
            "target": "Research:/report.pdf",
            "byte_len": 4096,
            "media_type": "application/pdf",
            "original_filename": "SECRET_ORIGINAL_FILENAME.pdf",
            "encryption_metadata": {"wrapped_key": "SECRET_ENCRYPTION_METADATA"}
        });
        execute_call(
            &state,
            Some(&caller),
            "file_transfer",
            &upload_input,
            async {
                Ok(CallToolResult::structured(serde_json::json!({
                    "upload_id": "upload-1",
                    "target": "Research:/report.pdf",
                    "transfer": {
                        "method": "PUT",
                        "url": "SECRET_UPLOAD_URL",
                        "headers": {"authorization": "SECRET_UPLOAD_HEADER"},
                        "expires_in_seconds": 300
                    },
                    "next_action": {
                        "kind": "http_upload",
                        "instruction": "SECRET_UPLOAD_INSTRUCTION",
                        "transfer_field": "transfer"
                    }
                }))
                .into())
            },
        )
        .await?;

        let me_input = serde_json::json!({
            "purpose": "SECRET_ME_PURPOSE",
            "unexpected": "SECRET_ME_ARGUMENT"
        });
        execute_call(&state, Some(&caller), "me", &me_input, async {
            Ok(CallToolResult::structured(serde_json::json!({
                "account": {
                    "id": "account-1",
                    "kind": "user",
                    "display_name": "SECRET_DISPLAY_NAME"
                },
                "user": {"email": "SECRET_EMAIL"},
                "capabilities": {"can_create_space": true, "can_manage_agents": true},
                "server_version": "0.1.50"
            }))
            .into())
        })
        .await?;

        let sequence_input = serde_json::json!({
            "purpose": "run a sequence with sensitive nested data",
            "commands": [
                {"tool": "search", "op": "grep", "target": "Research:/", "q": "SECRET_SEQUENCE_QUERY"},
                {"tool": "read", "op": "read", "target": "Research:/secret.md"},
                {"tool": "write", "op": "patch", "target": "Research:/note.md", "edits": [{
                    "old_text": "SECRET_SEQUENCE_OLD",
                    "new_text": "SECRET_SEQUENCE_NEW",
                    "mode": "unique"
                }]}
            ]
        });
        execute_call(
            &state,
            Some(&caller),
            "run_sequence",
            &sequence_input,
            async {
                Ok(CallToolResult::structured(serde_json::json!({
                    "ok": false,
                    "completed": 2,
                    "failed_index": 2,
                    "results": [
                        {
                            "index": 0,
                            "tool": "search",
                            "op": "grep",
                            "ok": true,
                            "result": {
                                "space": "Research",
                                "items": [{
                                    "path": "/secret.md",
                                    "match_lines": ["1: SECRET_SEQUENCE_MATCH_LINE"]
                                }],
                                "page": {"limit": 20, "returned": 1, "has_more": false}
                            }
                        },
                        {
                            "index": 1,
                            "tool": "read",
                            "op": "read",
                            "ok": true,
                            "result": {
                                "space": "Research",
                                "path": "/secret.md",
                                "content": "SECRET_SEQUENCE_CONTENT",
                                "content_sha256": "hash"
                            }
                        }
                    ],
                    "error": {
                        "code": -32602,
                        "message": "SECRET_SEQUENCE_ERROR",
                        "data": {
                            "code": "invalid_input",
                            "recoverable": true,
                            "next_action": {
                                "kind": "retry",
                                "hint": "SECRET_SEQUENCE_HINT",
                                "input": {
                                    "tool": "search",
                                    "op": "grep",
                                    "target": "Research:/",
                                    "q": "SECRET_SEQUENCE_RETRY_QUERY"
                                }
                            }
                        }
                    }
                }))
                .into())
            },
        )
        .await?;

        let rows = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                serde_json::Value,
                serde_json::Value,
                String,
                Option<String>,
            ),
        >(
            "SELECT tool, op, input, response, outcome, error_code \
             FROM mcp_invocations WHERE actor_account_id = $1 ORDER BY id",
        )
        .bind(caller.account_id())
        .fetch_all(&state.db)
        .await?;
        assert_eq!(rows.len(), 3);

        let persisted = serde_json::to_string(&rows)?;
        for secret in [
            "SECRET_ORIGINAL_FILENAME",
            "SECRET_ENCRYPTION_METADATA",
            "SECRET_UPLOAD_URL",
            "SECRET_UPLOAD_HEADER",
            "SECRET_UPLOAD_INSTRUCTION",
            "SECRET_ME_PURPOSE",
            "SECRET_ME_ARGUMENT",
            "SECRET_DISPLAY_NAME",
            "SECRET_EMAIL",
            "SECRET_SEQUENCE_QUERY",
            "SECRET_SEQUENCE_OLD",
            "SECRET_SEQUENCE_NEW",
            "SECRET_SEQUENCE_MATCH_LINE",
            "SECRET_SEQUENCE_CONTENT",
            "SECRET_SEQUENCE_ERROR",
            "SECRET_SEQUENCE_HINT",
            "SECRET_SEQUENCE_RETRY_QUERY",
        ] {
            assert!(!persisted.contains(secret), "persisted secret: {secret}");
        }

        assert_eq!(rows[0].0, "file_transfer");
        assert_eq!(rows[0].1.as_deref(), Some("begin_upload"));
        assert_eq!(
            rows[0].2["original_filename"]["category"],
            "original_filename"
        );
        assert_eq!(
            rows[0].3["result"]["transfer"]["url"]["category"],
            "presigned_url"
        );
        assert_eq!(
            rows[0].3["result"]["transfer"]["headers"]["category"],
            "transfer_headers"
        );
        assert_eq!(rows[0].4, "success");

        assert_eq!(rows[1].0, "me");
        assert_eq!(rows[1].2["_omitted_field_count"], 2);
        assert_eq!(
            rows[1].3["result"]["account"]["display_name"]["category"],
            "pii"
        );
        assert_eq!(rows[1].3["result"]["user"]["email"]["category"], "pii");
        assert_eq!(rows[1].4, "success");

        assert_eq!(rows[2].0, "run_sequence");
        assert_eq!(rows[2].2["commands"][0]["q"]["category"], "search_query");
        assert_eq!(
            rows[2].2["commands"][2]["edits"][0]["old_text"]["category"],
            "document_content"
        );
        assert_eq!(
            rows[2].3["result"]["results"][0]["result"]["items"][0]["match_lines"]["category"],
            "document_content"
        );
        assert_eq!(
            rows[2].3["result"]["results"][1]["result"]["content"]["category"],
            "document_content"
        );
        assert_eq!(
            rows[2].3["result"]["error"]["data"]["next_action"]["input"]["q"]["category"],
            "search_query"
        );
        assert_eq!(rows[2].4, "error");
        assert_eq!(rows[2].5.as_deref(), Some("invalid_input"));

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
        let input = serde_json::json!({
            "purpose": "search the owner's notes",
            "op": "grep",
            "target": "Research:/",
            "q": "cache"
        });
        execute_call(&state, Some(&agent_caller), "search", &input, async {
            Ok(CallToolResult::structured(serde_json::json!({"items": []})).into())
        })
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
