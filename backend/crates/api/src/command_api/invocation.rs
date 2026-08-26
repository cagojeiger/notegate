//! CLI transport adapter with shared command dispatch and invocation capture.

use std::time::Instant;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use notegate_command::{
    COMMAND_PROTOCOL_VERSION, CommandError, CommandTool, FileDownloadInput, FileUploadInput,
    RecoveryAction, RunReadSequenceInput, RunWriteSequenceInput, validate_purpose,
};
use notegate_model::Caller;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::context::HttpCommandContext;
use super::error::CommandHttpError;
use crate::commands::{self, CommandContext};
use crate::invocations::redaction::{
    bounded_snapshot, redact_input, redact_output_value, redact_structured_response,
};
use crate::invocations::{
    InvocationRecord, InvocationSurface, canonical_op, invocation_space_name, record,
    sequence_error_code,
};
use crate::observability::CommandInvocationMetrics;
use crate::state::AppState;

const CLI_VERSION_HEADER: &str = "x-notegate-cli-version";
const COMMAND_PROTOCOL_HEADER: &str = "x-notegate-command-protocol";

pub(super) type RawJsonInput = Result<Json<Value>, JsonRejection>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CliInvocationEnvelope {
    tool: String,
    input: Value,
}

pub(super) async fn execute(
    state: AppState,
    context: HttpCommandContext,
    headers: &HeaderMap,
    input: RawJsonInput,
) -> Result<Json<Value>, CommandHttpError> {
    let started = Instant::now();
    let command_context = context.into_command();
    let caller = command_context.caller().clone();

    let raw_envelope = match input {
        Ok(Json(input)) => input,
        Err(rejection) => {
            let metrics =
                CommandInvocationMetrics::start(state.config.metrics_enabled, "cli", "unknown");
            let result = Err(CommandHttpError::invalid_json(rejection));
            return finish(
                &state,
                &caller,
                "unknown",
                &Value::Null,
                result,
                started,
                metrics,
            )
            .await;
        }
    };

    let raw_tool = raw_envelope
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let tool = CommandTool::parse(raw_tool).map_or("unknown", CommandTool::as_str);
    let raw_input = raw_envelope.get("input").cloned().unwrap_or(Value::Null);
    let metrics = CommandInvocationMetrics::start(state.config.metrics_enabled, "cli", tool);

    if let Err(error) = validate_command_protocol(headers) {
        return finish(
            &state,
            &caller,
            tool,
            &raw_input,
            Err(error),
            started,
            metrics,
        )
        .await;
    }

    let envelope = match serde_json::from_value::<CliInvocationEnvelope>(raw_envelope) {
        Ok(envelope) => envelope,
        Err(error) => {
            let result = Err(CommandHttpError::invalid_schema(error));
            return finish(&state, &caller, tool, &raw_input, result, started, metrics).await;
        }
    };

    let known_tool = match CommandTool::parse(&envelope.tool) {
        Some(tool) => tool,
        None => {
            let result = Err(CommandHttpError::from(unknown_tool_error(&envelope.tool)));
            return finish(
                &state,
                &caller,
                "unknown",
                &envelope.input,
                result,
                started,
                metrics,
            )
            .await;
        }
    };
    let tool = known_tool.as_str();
    let result = dispatch(&state, command_context, known_tool, envelope.input.clone())
        .await
        .map_err(Into::into);
    finish(
        &state,
        &caller,
        tool,
        &envelope.input,
        result,
        started,
        metrics,
    )
    .await
}

async fn dispatch(
    state: &AppState,
    context: CommandContext,
    tool: CommandTool,
    input: Value,
) -> Result<Value, CommandError> {
    match tool {
        CommandTool::Me => {
            parse_empty_input(input)?;
            serde_json::to_value(commands::identity::call(&context))
                .map_err(|_| CommandError::internal("could not serialize identity response"))
        }
        CommandTool::Read => commands::executor::read(state, &context, parse_input(input)?).await,
        CommandTool::Search => {
            commands::executor::search(state, &context, parse_input(input)?).await
        }
        CommandTool::Write => commands::executor::write(state, &context, parse_input(input)?).await,
        CommandTool::Manage => {
            commands::executor::manage(state, &context, parse_input(input)?).await
        }
        CommandTool::FileDownload => {
            commands::transfers::download(state, &context, parse_input::<FileDownloadInput>(input)?)
                .await
        }
        CommandTool::FileUpload => {
            commands::transfers::upload(state, &context, parse_input::<FileUploadInput>(input)?)
                .await
        }
        CommandTool::RunReadSequence => {
            commands::sequence::run_read(
                state,
                &context,
                parse_input::<RunReadSequenceInput>(input)?,
            )
            .await
        }
        CommandTool::RunWriteSequence => {
            commands::sequence::run_write(
                state,
                &context,
                parse_input::<RunWriteSequenceInput>(input)?,
            )
            .await
        }
    }
}

fn parse_input<T: DeserializeOwned>(input: Value) -> Result<T, CommandError> {
    serde_json::from_value(input).map_err(|error| invalid_schema_error(error.to_string()))
}

fn parse_empty_input(input: Value) -> Result<(), CommandError> {
    if input.as_object().is_some_and(serde_json::Map::is_empty) {
        return Ok(());
    }
    Err(commands::error::actionable_input_error(
        "me_input_must_be_empty",
        "me input must be an empty object",
        "Remove every field from input and retry me.",
        RecoveryAction::ReplaceField {
            field: "input".to_owned(),
            value: json!({}),
        },
    ))
}

fn invalid_schema_error(detail: String) -> CommandError {
    CommandError::invalid_params("command input does not match the shared schema").with_data(
        json!({
            "kind": "invalid_input",
            "code": "invalid_json",
            "detail": detail,
        }),
    )
}

fn unknown_tool_error(tool: &str) -> CommandError {
    commands::error::actionable_input_error(
        "unknown_tool",
        format!("unknown CLI tool '{tool}'"),
        "Choose one of the tool values listed by next_action.choices.",
        RecoveryAction::ChooseValue {
            field: "tool".to_owned(),
            choices: CommandTool::ALL
                .into_iter()
                .map(|tool| json!(tool.as_str()))
                .collect(),
        },
    )
}

fn validate_command_protocol(headers: &HeaderMap) -> Result<(), CommandHttpError> {
    let client_version = headers
        .get(CLI_VERSION_HEADER)
        .and_then(|value| value.to_str().ok());
    let client_protocol = headers
        .get(COMMAND_PROTOCOL_HEADER)
        .and_then(|value| value.to_str().ok());
    if client_protocol == Some(COMMAND_PROTOCOL_VERSION) {
        return Ok(());
    }
    Err(CommandHttpError::cli_update_required(
        client_version,
        client_protocol,
    ))
}

async fn finish(
    state: &AppState,
    caller: &Caller,
    tool: &str,
    input: &Value,
    result: Result<Value, CommandHttpError>,
    started: Instant,
    metrics: CommandInvocationMetrics,
) -> Result<Json<Value>, CommandHttpError> {
    let elapsed = started.elapsed();
    let metadata = InvocationMetadata::from_input(tool, input);
    let redacted_input = redact_input(tool, input);
    let error_code = result
        .as_ref()
        .err()
        .map(|error| error.error_code().to_owned())
        .or_else(|| result.as_ref().ok().and_then(sequence_error_code));
    let outcome = if error_code.is_some() {
        "error"
    } else {
        "success"
    };
    metrics.finish(outcome, elapsed);

    let response = redact_response(tool, &metadata.response_context, &result);
    record(
        state,
        caller,
        InvocationRecord {
            surface: InvocationSurface::Cli,
            tool,
            op: metadata.op.as_deref(),
            purpose: metadata.purpose.as_deref(),
            space_name: metadata.space_name.as_deref(),
            input: &redacted_input,
            response: Some(&response),
            error_code: error_code.as_deref(),
            elapsed_ms: elapsed.as_millis(),
        },
    )
    .await;

    result.map(Json)
}

struct InvocationMetadata {
    op: Option<String>,
    purpose: Option<String>,
    space_name: Option<String>,
    response_context: Value,
}

impl InvocationMetadata {
    fn from_input(tool: &str, input: &Value) -> Self {
        let op = canonical_op(tool, input.get("op").and_then(Value::as_str)).map(str::to_owned);
        let purpose = if tool == "me" {
            None
        } else {
            input
                .get("purpose")
                .and_then(Value::as_str)
                .filter(|purpose| validate_purpose(purpose).is_ok())
                .map(str::to_owned)
        };
        let space_name = if tool == "read" && op.as_deref() == Some("changes") {
            invocation_space_name(input.get("target").and_then(Value::as_str))
        } else {
            None
        };
        let response_context = op
            .as_ref()
            .map_or_else(|| json!({}), |op| json!({"op": op}));
        Self {
            op,
            purpose,
            space_name,
            response_context,
        }
    }
}

fn redact_response(tool: &str, input: &Value, result: &Result<Value, CommandHttpError>) -> Value {
    let snapshot = match result {
        Ok(value) => json!({
            "kind": "complete",
            "is_error": false,
            "result": redact_structured_response(tool, input, value),
        }),
        Err(error) => {
            let mut body = Map::new();
            body.insert(
                "code".to_owned(),
                Value::String(error.error_code().to_owned()),
            );
            body.insert("kind".to_owned(), Value::String(error.kind().to_owned()));
            if let Some(data) = error.data() {
                body.insert("data".to_owned(), redact_output_value(data));
            }
            json!({"kind": "error", "error": body})
        }
    };
    bounded_snapshot(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mcp_command_name_is_accepted_by_the_cli_dispatch() {
        let names = CommandTool::ALL.map(CommandTool::as_str);
        assert_eq!(
            names,
            [
                "me",
                "read",
                "search",
                "write",
                "manage",
                "file_download",
                "file_upload",
                "run_read_sequence",
                "run_write_sequence",
            ]
        );
    }

    #[test]
    fn unknown_tool_error_lists_the_complete_shared_surface() {
        let error = unknown_tool_error("reads");
        let data = error.data.as_ref();
        assert_eq!(
            data.and_then(|value| value.get("code"))
                .and_then(Value::as_str),
            Some("unknown_tool")
        );
        assert_eq!(
            data.and_then(|value| value.pointer("/next_action/choices"))
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(9)
        );
    }

    #[test]
    fn me_metadata_never_extracts_a_purpose_from_rejected_input() {
        let metadata =
            InvocationMetadata::from_input("me", &json!({"purpose": "must not be persisted"}));

        assert_eq!(metadata.purpose, None);
    }
}
