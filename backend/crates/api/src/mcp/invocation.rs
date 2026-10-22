//! Shared MCP tool execution and best-effort invocation history capture.

use std::future::Future;
use std::time::Instant;

use notegate_command::CommandTool;
use notegate_model::Caller;
use rmcp::ErrorData;
use rmcp::model::{CallToolResponse, CallToolResult};
use serde_json::{Map, Value, json};

use crate::commands;
use crate::invocations::redaction::{
    bounded_snapshot, redact_input, redact_output_value, redact_structured_response,
    redaction_marker,
};
use crate::invocations::{
    InvocationCapture, InvocationCompletion, InvocationMetadata, InvocationRecord,
    InvocationSurface, canonical_tool, sequence_error_code,
};
use crate::mcp::contract::command_error;
use crate::observability::CommandInvocationMetrics;

pub(crate) async fn execute_call(
    invocation_id: uuid::Uuid,
    capture: InvocationCapture<'_>,
    caller: Option<&Caller>,
    tool: &str,
    input: &Value,
    future: impl Future<Output = Result<CallToolResponse, ErrorData>>,
) -> Result<CallToolResponse, ErrorData> {
    let completion_started = Instant::now();
    let metadata = InvocationMetadata::from_input(tool, input);
    let purpose_validation = metadata
        .validate_purpose()
        .map_err(|error| invalid_input_error(error.to_string()));

    let started = Instant::now();
    let metrics =
        CommandInvocationMetrics::start(capture.metrics_enabled, "mcp", canonical_tool(tool));
    let result = match purpose_validation {
        Ok(_) => future.await,
        Err(error) => Err(error),
    };
    let error_code = call_error_code(tool, &result);
    let elapsed = started.elapsed();
    let completion =
        InvocationCompletion::new(metrics, completion_started, elapsed, error_code.as_deref());

    if let Some(caller) = caller {
        let redacted_input = redact_input(tool, input);
        let redacted_response = redact_mcp_response(tool, input, &result);
        completion
            .record(
                capture.writer,
                caller,
                InvocationRecord {
                    id: Some(invocation_id),
                    surface: InvocationSurface::Mcp,
                    tool: canonical_tool(tool),
                    op: metadata.op,
                    purpose: metadata.purpose(),
                    space_name: metadata.space_name.as_deref(),
                    input: &redacted_input,
                    response: Some(&redacted_response),
                    error_code: error_code.as_deref(),
                    elapsed_ms: elapsed.as_millis(),
                },
            )
            .await;
    } else {
        completion.finish();
    }

    result
}

pub(crate) fn redact_mcp_response(
    tool: &str,
    input: &Value,
    result: &Result<CallToolResponse, ErrorData>,
) -> Value {
    let snapshot = match result {
        Err(error) => json!({"kind": "error", "error": redact_mcp_error(error)}),
        Ok(CallToolResponse::Complete(result)) => complete_response(tool, input, result),
        Ok(CallToolResponse::InputRequired(_)) => unsupported_response("input_required"),
        Ok(CallToolResponse::Task(_)) => unsupported_response("task"),
        Ok(_) => unsupported_response("unknown"),
    };
    bounded_snapshot(snapshot)
}

fn complete_response(tool: &str, input: &Value, result: &CallToolResult) -> Value {
    let mut snapshot = Map::new();
    snapshot.insert("kind".to_owned(), Value::String("complete".to_owned()));
    snapshot.insert(
        "is_error".to_owned(),
        Value::Bool(result.is_error.unwrap_or(false)),
    );
    snapshot.insert(
        "content_blocks_omitted".to_owned(),
        json!(result.content.len()),
    );
    if let Some(structured) = result.structured_content.as_ref() {
        snapshot.insert(
            "result".to_owned(),
            redact_structured_response(tool, input, structured),
        );
    }
    Value::Object(snapshot)
}

fn unsupported_response(kind: &str) -> Value {
    json!({
        "kind": kind,
        "details": redaction_marker("unsupported_response_variant", &Value::Null),
    })
}

fn redact_mcp_error(error: &ErrorData) -> Value {
    let mut output = Map::new();
    output.insert("code".to_owned(), json!(error.code.0));
    output.insert(
        "message".to_owned(),
        redaction_marker(
            "untrusted_error_text",
            &Value::String(error.message.to_string()),
        ),
    );
    if let Some(data) = error.data.as_ref() {
        output.insert("data".to_owned(), redact_output_value(data));
    }
    Value::Object(output)
}

fn invalid_input_error(message: impl Into<String>) -> ErrorData {
    command_error(commands::error::invalid_input_error(message))
}

fn call_error_code(tool: &str, result: &Result<CallToolResponse, ErrorData>) -> Option<String> {
    match result {
        Err(error) => Some(error_code(error)),
        Ok(CallToolResponse::Complete(result)) if result.is_error == Some(true) => {
            Some(tool_result_error_code(result))
        }
        Ok(CallToolResponse::Complete(result))
            if CommandTool::parse(tool).is_some_and(CommandTool::is_sequence) =>
        {
            result
                .structured_content
                .as_ref()
                .and_then(sequence_error_code)
        }
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
    use crate::invocations::invocation_space_name;
    use crate::invocations::test_support::{
        RecordingWriter, assert_completed, capture, report, user_caller,
    };
    use notegate_model::CallerIdentity;

    #[tokio::test]
    async fn completion_separates_command_and_history_outcomes()
    -> Result<(), Box<dyn std::error::Error>> {
        let writer = RecordingWriter::default();
        let capture_state = InvocationCapture {
            writer: &writer,
            metrics_enabled: true,
        };
        let caller = user_caller();
        let input = json!({});
        for history_outcome in ["success", "error"] {
            writer.fail_writes(history_outcome == "error");
            for outcome in ["success", "error"] {
                let (response, body) = capture(execute_call(
                    uuid::Uuid::new_v4(),
                    capture_state,
                    Some(&caller),
                    "me",
                    &input,
                    async {
                        if outcome == "success" {
                            Ok(CallToolResult::structured(json!({"ok": true})).into())
                        } else {
                            Err(ErrorData::invalid_params("invalid input", None))
                        }
                    },
                ))
                .await;
                assert_eq!(response.is_ok(), outcome == "success");
                assert_completed(&body, outcome, history_outcome);
            }
        }
        let (response, body) = capture(execute_call(
            uuid::Uuid::new_v4(),
            capture_state,
            None,
            "me",
            &input,
            async { Ok(CallToolResult::structured(json!({})).into()) },
        ))
        .await;
        response?;
        assert!(
            !body.contains("notegate_command_history_records_total"),
            "{body}"
        );
        assert!(
            body.contains("notegate_command_completion_duration_seconds_count"),
            "{body}"
        );
        assert_eq!(
            writer.attempts(),
            4,
            "anonymous calls must not attempt history writes"
        );
        assert_eq!(writer.records().len(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn completion_waits_for_history_and_preserves_invocation_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::{AtomicBool, Ordering};

        use crate::invocations::{InvocationWriteFuture, InvocationWriter};

        struct GatedWriter {
            recording: RecordingWriter,
            entered: AtomicBool,
            release: tokio::sync::Semaphore,
        }

        impl InvocationWriter for GatedWriter {
            fn write<'a>(
                &'a self,
                invocation: notegate_db::NewCommandInvocation<'a>,
            ) -> InvocationWriteFuture<'a> {
                Box::pin(async move {
                    self.entered.store(true, Ordering::SeqCst);
                    let _permit = self
                        .release
                        .acquire()
                        .await
                        .expect("history write released");
                    self.recording.write(invocation).await
                })
            }
        }

        let writer = GatedWriter {
            recording: RecordingWriter::default(),
            entered: AtomicBool::new(false),
            release: tokio::sync::Semaphore::new(0),
        };
        let mut caller = user_caller();
        let owner = caller.account_id();
        let actor = uuid::Uuid::new_v4();
        caller.account.id = actor;
        caller.account.kind = notegate_model::account::AccountKind::Agent;
        caller.identity = CallerIdentity::Agent(notegate_model::agent::Agent {
            id: actor,
            name: "history-agent".to_owned(),
            owner_user_id: owner,
        });
        let id = uuid::Uuid::new_v4();
        let input = json!({"op": "changes", "target": "Research:/", "purpose": "Review changes"});
        let executed = AtomicBool::new(false);
        let mut completion = std::pin::pin!(capture(execute_call(
            id,
            InvocationCapture {
                writer: &writer,
                metrics_enabled: true
            },
            Some(&caller),
            "read",
            &input,
            async {
                executed.store(true, Ordering::SeqCst);
                Ok(CallToolResult::structured(json!({"events": []})).into())
            },
        )));

        assert!(futures_util::poll!(completion.as_mut()).is_pending());
        assert!(executed.load(Ordering::SeqCst));
        assert!(writer.entered.load(Ordering::SeqCst));
        assert!(writer.recording.records().is_empty());

        writer.release.add_permits(1);
        let (response, metrics) = completion.await;
        response?;
        assert_completed(&metrics, "success", "success");
        let item = writer
            .recording
            .records()
            .pop()
            .expect("recorded invocation");
        assert_eq!(writer.recording.attempts(), 1);
        assert_eq!(item.invocation_id, Some(id));
        assert_eq!(item.owner_user_id, owner);
        assert_eq!(item.actor_account_id, actor);
        assert_eq!(item.caller_kind, "agent");
        assert_eq!(item.surface, "mcp");
        assert_eq!(item.tool, "read");
        assert_eq!(item.op.as_deref(), Some("changes"));
        assert_eq!(item.purpose.as_deref(), Some("Review changes"));
        assert_eq!(item.space_name.as_deref(), Some("Research"));
        Ok(())
    }

    #[tokio::test]
    #[ignore = "diagnostic adapter/history timings; run separately in CI"]
    async fn measure_command_completion() -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let mut state = crate::rest::test_support::state(&db);
        std::sync::Arc::make_mut(&mut state.config).metrics_enabled = true;
        let (caller, _, _) = crate::rest::test_support::caller_and_space(&state).await?;
        let context = commands::CommandContext::new(caller.clone(), None);
        for scenario in ["me", "read_1mib_response"] {
            let input = if scenario == "me" {
                json!({})
            } else {
                json!({"op": "read", "target": "rest-test:/note.md", "purpose": "measure history capture"})
            };
            let mut samples = Vec::new();
            for iteration in 0..30 {
                let tool = if scenario == "me" { "me" } else { "read" };
                // A prebuilt response isolates history overhead from document retrieval.
                let response = if scenario == "me" {
                    serde_json::to_value(commands::identity::call(&context))?
                } else {
                    json!({"content": "x".repeat(1024 * 1024), "byte_len": 1024 * 1024})
                };
                let response = CallToolResult::structured(response).into();
                let (result, body) = capture(execute_call(
                    uuid::Uuid::new_v4(),
                    state.invocation_capture(),
                    Some(&caller),
                    tool,
                    &input,
                    async { Ok(response) },
                ))
                .await;
                result?;
                assert_completed(&body, "success", "success");
                if iteration >= 5 {
                    samples.push(body);
                }
            }
            report("mcp", scenario, &samples);
        }
        db.cleanup().await;
        Ok(())
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
            "completed": 0,
            "failed": 1,
            "skipped": 0,
            "results": [{
                "index": 0,
                "tool": "read",
                "op": "read",
                "ok": false,
                "error": {
                    "code": -32602,
                    "data": {"code": "invalid_input"}
                }
            }]
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
        let writer = RecordingWriter::default();
        let capture_state = InvocationCapture {
            writer: &writer,
            metrics_enabled: false,
        };
        let caller = user_caller();
        let search_input = serde_json::json!({
            "purpose": "locate cache design notes",
            "op": "find",
            "target": "Research:/",
            "q": "SECRET_SEARCH_QUERY"
        });
        execute_call(
            uuid::Uuid::new_v4(),
            capture_state,
            Some(&caller),
            "search",
            &search_input,
            async { Ok(CallToolResult::structured(serde_json::json!({"items": []})).into()) },
        )
        .await?;
        let missing_input = serde_json::json!({
            "purpose": "read a missing design note",
            "op": "read",
            "target": "Research:/missing.md"
        });
        let recorded_error = execute_call(
            uuid::Uuid::new_v4(),
            capture_state,
            Some(&caller),
            "read",
            &missing_input,
            async {
                Err::<CallToolResponse, _>(invalid_input_error(
                    "the requested design note does not exist",
                ))
            },
        )
        .await
        .expect_err("tool error is returned");
        assert_eq!(recorded_error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        let invalid_purpose_input = serde_json::json!({"purpose": " ", "op": "spaces"});
        let executed = std::sync::atomic::AtomicBool::new(false);
        let error = execute_call(
            uuid::Uuid::new_v4(),
            capture_state,
            Some(&caller),
            "read",
            &invalid_purpose_input,
            async {
                executed.store(true, std::sync::atomic::Ordering::Relaxed);
                Ok(CallToolResult::structured(serde_json::json!({"spaces": []})).into())
            },
        )
        .await
        .expect_err("blank purpose is rejected");
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
        let sequence_input = serde_json::json!({
            "purpose": "run a failing sequence",
            "commands": [{"tool": "read", "op": "read", "target": "Research:/missing.md"}]
        });
        let sequence = execute_call(
            uuid::Uuid::new_v4(),
            capture_state,
            Some(&caller),
            "run_read_sequence",
            &sequence_input,
            async {
                Ok(CallToolResult::structured(serde_json::json!({
                    "ok": false,
                    "completed": 0,
                    "failed": 1,
                    "skipped": 0,
                    "results": [{
                        "index": 0,
                        "tool": "read",
                        "op": "read",
                        "ok": false,
                        "error": {
                            "code": -32602,
                            "message": "invalid input",
                            "data": {"code": "invalid_input"}
                        }
                    }]
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

        let recorded = writer.records();
        assert_eq!(writer.attempts(), 4);
        for item in &recorded {
            assert_eq!(item.owner_user_id, caller.account_id());
            assert_eq!(item.actor_account_id, caller.account_id());
            assert_eq!(item.caller_kind, "user");
            assert_eq!(item.surface, "mcp");
            assert!(item.invocation_id.is_some());
            assert!(item.space_name.is_none());
            assert!(item.duration_ms >= 0);
        }
        let rows: Vec<_> = recorded
            .into_iter()
            .map(|item| {
                (
                    item.tool,
                    item.op,
                    item.purpose,
                    item.input,
                    item.response.expect("captured response"),
                    item.outcome,
                    item.error_code,
                )
            })
            .collect();
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
        assert_eq!(rows[3].0, "run_read_sequence");
        assert_eq!(rows[3].2.as_deref(), Some("run a failing sequence"));
        assert_eq!(rows[3].3, sequence_input);
        assert_eq!(rows[3].4["result"]["ok"], false);
        assert_eq!(rows[3].5, "error");
        assert_eq!(rows[3].6.as_deref(), Some("invalid_input"));

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
            uuid::Uuid::new_v4(),
            state.invocation_capture(),
            Some(&caller),
            "file_upload",
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
        execute_call(
            uuid::Uuid::new_v4(),
            state.invocation_capture(),
            Some(&caller),
            "me",
            &me_input,
            async {
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
            },
        )
        .await?;

        let read_sequence_input = serde_json::json!({
            "purpose": "read sensitive nested data",
            "commands": [
                {"tool": "search", "op": "grep", "target": "Research:/", "q": "SECRET_SEQUENCE_QUERY"},
                {"tool": "read", "op": "read", "target": "Research:/secret.md"}
            ]
        });
        execute_call(
            uuid::Uuid::new_v4(),
            state.invocation_capture(),
            Some(&caller),
            "run_read_sequence",
            &read_sequence_input,
            async {
                Ok(CallToolResult::structured(serde_json::json!({
                    "ok": true,
                    "completed": 2,
                    "failed": 0,
                    "skipped": 0,
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
                    ]
                }))
                .into())
            },
        )
        .await?;

        let write_sequence_input = serde_json::json!({
            "purpose": "attempt a sensitive patch",
            "commands": [
                {"tool": "write", "op": "patch", "target": "Research:/note.md", "edits": [{
                    "old_text": "SECRET_SEQUENCE_OLD",
                    "new_text": "SECRET_SEQUENCE_NEW",
                    "mode": "unique"
                }]}
            ]
        });
        execute_call(
            uuid::Uuid::new_v4(),
            state.invocation_capture(),
            Some(&caller),
            "run_write_sequence",
            &write_sequence_input,
            async {
                Ok(CallToolResult::structured(serde_json::json!({
                    "ok": false,
                    "completed": 0,
                    "failed": 1,
                    "skipped": 0,
                    "results": [{
                        "index": 0,
                        "tool": "write",
                        "op": "patch",
                        "ok": false,
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
                                        "tool": "write",
                                        "op": "patch",
                                        "target": "Research:/note.md",
                                        "edits": [{
                                            "old_text": "SECRET_SEQUENCE_RETRY_OLD",
                                            "new_text": "SECRET_SEQUENCE_RETRY_NEW"
                                        }]
                                    }
                                }
                            }
                        }
                    }]
                }))
                .into())
            },
        )
        .await?;

        let mut rows: Vec<_> = state
            .command_invocations
            .list_by_owner(
                caller.account_id(),
                notegate_model::CommandInvocationSurface::Mcp,
                100,
                None,
            )
            .await?
            .into_iter()
            .map(|item| {
                (
                    item.tool,
                    item.op,
                    item.input,
                    item.response.expect("captured response"),
                    item.outcome,
                    item.error_code,
                )
            })
            .collect();
        rows.reverse();
        assert_eq!(rows.len(), 4);

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
            "SECRET_SEQUENCE_RETRY_OLD",
            "SECRET_SEQUENCE_RETRY_NEW",
        ] {
            assert!(!persisted.contains(secret), "persisted secret: {secret}");
        }

        assert_eq!(rows[0].0, "file_upload");
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

        assert_eq!(rows[2].0, "run_read_sequence");
        assert_eq!(rows[2].2["commands"][0]["q"]["category"], "search_query");
        assert_eq!(
            rows[2].3["result"]["results"][0]["result"]["items"][0]["match_lines"]["category"],
            "document_content"
        );
        assert_eq!(
            rows[2].3["result"]["results"][1]["result"]["content"]["category"],
            "document_content"
        );
        assert_eq!(rows[2].4, "success");
        assert_eq!(rows[2].5, None);

        assert_eq!(rows[3].0, "run_write_sequence");
        assert_eq!(
            rows[3].2["commands"][0]["edits"][0]["old_text"]["category"],
            "document_content"
        );
        assert_eq!(
            rows[3].3["result"]["results"][0]["error"]["data"]["next_action"]["input"]["edits"][0]
                ["old_text"]["category"],
            "document_content"
        );
        assert_eq!(rows[3].4, "error");
        assert_eq!(rows[3].5.as_deref(), Some("invalid_input"));

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
        execute_call(
            uuid::Uuid::new_v4(),
            state.invocation_capture(),
            Some(&agent_caller),
            "search",
            &input,
            async { Ok(CallToolResult::structured(serde_json::json!({"items": []})).into()) },
        )
        .await?;

        let row = sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid, String, String)>(
            "SELECT owner_user_id, actor_account_id, caller_kind, surface \
             FROM command_invocations WHERE actor_account_id = $1",
        )
        .bind(agent_account_id)
        .fetch_one(&state.db)
        .await?;
        assert_eq!(row.0, owner_user_id);
        assert_eq!(row.1, agent_account_id);
        assert_eq!(row.2, "agent");
        assert_eq!(row.3, "mcp");

        db.cleanup().await;
        Ok(())
    }
}
