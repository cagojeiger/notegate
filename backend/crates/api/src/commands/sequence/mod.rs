//! Transport-neutral sequence validation and execution.

use notegate_command::{
    CommandError, CommandErrorClass, RecoveryAction, RecoveryErrorData, RequiredField,
    RunReadSequenceInput, RunWriteSequenceInput, SEQUENCE_MAX_COMMANDS, SequenceCommand,
    SequenceKind,
};
use serde_json::{Value, json};

use super::error::{actionable_input_error, invalid_input_error, required_input};
use super::{CommandContext, executor};
use crate::state::AppState;

mod read;
mod write;

const READ_COMMAND_FIELDS: &[&str] = &[
    "tool",
    "op",
    "target",
    "name",
    "depth",
    "limit",
    "cursor",
    "direction",
    "start_line",
    "max_lines",
    "max_bytes",
    "if_none_match_sha256",
];
const SEARCH_COMMAND_FIELDS: &[&str] = &[
    "tool", "op", "target", "q", "kind", "match", "lines", "include", "exclude", "limit", "cursor",
];
const WRITE_COMMAND_FIELDS: &[&str] = &[
    "tool",
    "op",
    "target",
    "content",
    "edits",
    "create",
    "ensure_newline",
    "expected_sha256",
];
const MANAGE_COMMAND_FIELDS: &[&str] = &[
    "tool",
    "op",
    "target",
    "source",
    "destination",
    "parents",
    "recursive",
];

#[derive(Debug, Clone)]
struct PreparedSequenceCommand {
    index: usize,
    command: SequenceCommand,
}

struct SequenceOutcome {
    index: usize,
    tool: String,
    op: String,
    result: Result<Value, CommandError>,
}

pub async fn run_read(
    state: &AppState,
    context: &CommandContext,
    input: RunReadSequenceInput,
) -> Result<Value, CommandError> {
    validate_sequence_command_count(input.commands.len(), SequenceKind::Read)?;
    let commands = prepare_sequence_commands(input.commands, &input.purpose, SequenceKind::Read)?;
    let purpose = input.purpose;
    let outcomes = read::collect(commands, |command| {
        execute_sequence_command(state, context, command, &purpose)
    })
    .await;
    Ok(sequence_response(outcomes, 0))
}

pub async fn run_write(
    state: &AppState,
    context: &CommandContext,
    input: RunWriteSequenceInput,
) -> Result<Value, CommandError> {
    validate_sequence_command_count(input.commands.len(), SequenceKind::Write)?;
    let command_count = input.commands.len();
    let commands = prepare_sequence_commands(input.commands, &input.purpose, SequenceKind::Write)?;
    let (outcomes, skipped) = write::collect(commands, command_count, |command| {
        execute_sequence_command(state, context, command, &input.purpose)
    })
    .await;
    Ok(sequence_response(outcomes, skipped))
}

fn command_fields(tool: &str) -> Option<&'static [&'static str]> {
    match tool {
        "read" => Some(READ_COMMAND_FIELDS),
        "search" => Some(SEARCH_COMMAND_FIELDS),
        "write" => Some(WRITE_COMMAND_FIELDS),
        "manage" => Some(MANAGE_COMMAND_FIELDS),
        _ => None,
    }
}

fn is_command_field(field: &str) -> bool {
    [
        READ_COMMAND_FIELDS,
        SEARCH_COMMAND_FIELDS,
        WRITE_COMMAND_FIELDS,
        MANAGE_COMMAND_FIELDS,
    ]
    .into_iter()
    .any(|fields| fields.contains(&field))
}

fn prepare_sequence_commands(
    commands: Vec<Value>,
    purpose: &str,
    kind: SequenceKind,
) -> Result<Vec<PreparedSequenceCommand>, CommandError> {
    let mut prepared = Vec::with_capacity(commands.len());
    let mut issues = Vec::new();

    for (index, value) in commands.into_iter().enumerate() {
        let Some(object) = value.as_object() else {
            issues.push(sequence_issue(
                index,
                "sequence_command_must_be_object",
                "sequence commands must be JSON objects",
                "Replace this command with a flat object containing tool, op, and operation fields.",
                None,
            ));
            continue;
        };
        let mut candidate = object.clone();
        let mut shape_blocked = false;

        if object.contains_key("purpose") {
            issues.push(sequence_issue(
                index,
                "sequence_command_purpose_not_allowed",
                format!(
                    "purpose belongs to the {} invocation, not an internal command",
                    kind.tool_name()
                ),
                "Remove this field; every command inherits the one top-level invocation purpose.",
                Some(RecoveryAction::RemoveFields {
                    fields: vec![format!("commands[{index}].purpose")],
                }),
            ));
            candidate.remove("purpose");
        }

        if object.contains_key("args") {
            let replacement = flattened_sequence_command(&candidate);
            if let Some(Value::Object(flattened)) = replacement.as_ref() {
                candidate = flattened.clone();
            } else {
                shape_blocked = true;
            }
            issues.push(sequence_issue(
                index,
                "sequence_command_args_not_allowed",
                "sequence commands are flat objects and do not use an args wrapper",
                "Move every field from args into this command object, then remove args.",
                replacement.map(|value| RecoveryAction::ReplaceField {
                    field: format!("commands[{index}]"),
                    value,
                }),
            ));
        }

        let unknown_fields = candidate
            .keys()
            .filter(|field| !is_command_field(field))
            .cloned()
            .collect::<Vec<_>>();
        if !unknown_fields.is_empty() {
            issues.push(sequence_issue(
                index,
                "sequence_command_unknown_fields",
                "sequence command contains unsupported fields",
                "Remove every field listed by next_action.fields and retry.",
                Some(RecoveryAction::RemoveFields {
                    fields: unknown_fields
                        .iter()
                        .map(|field| format!("commands[{index}].{field}"))
                        .collect(),
                }),
            ));
            for field in unknown_fields {
                candidate.remove(&field);
            }
        }

        let tool_choices = kind.allowed_tools().join(", ");
        let missing_fields = [
            ("tool", format!("One of: {tool_choices}.")),
            (
                "op",
                format!("Allowed values depend on tool: {}", kind.operation_help()),
            ),
        ]
        .into_iter()
        .filter(|(field, _)| !candidate.contains_key(*field))
        .map(|(field, description)| RequiredField {
            field: format!("commands[{index}].{field}"),
            description: Some(description),
        })
        .collect::<Vec<_>>();
        if !missing_fields.is_empty() {
            issues.push(sequence_issue(
                index,
                "sequence_command_required_fields_missing",
                "sequence command is missing required fields",
                "Add every field listed by next_action.fields and retry.",
                Some(RecoveryAction::AddFields {
                    fields: missing_fields,
                }),
            ));
            shape_blocked = true;
        }

        if let Some(tool) = candidate.get("tool").and_then(Value::as_str)
            && let Some(allowed_fields) = command_fields(tool)
        {
            let disallowed_fields = candidate
                .keys()
                .filter(|field| !allowed_fields.contains(&field.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            if !disallowed_fields.is_empty() {
                issues.push(sequence_issue(
                    index,
                    "sequence_command_fields_not_allowed_for_tool",
                    format!("sequence {tool} command contains fields for another tool"),
                    "Remove every field listed by next_action.fields, or change tool to match the command shape.",
                    Some(RecoveryAction::RemoveFields {
                        fields: disallowed_fields
                            .iter()
                            .map(|field| format!("commands[{index}].{field}"))
                            .collect(),
                    }),
                ));
                for field in disallowed_fields {
                    candidate.remove(&field);
                }
            }
        }

        if shape_blocked {
            continue;
        }

        let command = match serde_json::from_value::<SequenceCommand>(Value::Object(candidate)) {
            Ok(command) => command,
            Err(error) => {
                issues.push(sequence_issue(
                    index,
                    "sequence_command_invalid_value",
                    format!("invalid sequence command: {error}"),
                    "Correct the field type or value at the reported command index and retry.",
                    None,
                ));
                continue;
            }
        };

        match validate_sequence_command(&command, purpose, kind) {
            Ok(()) => prepared.push(PreparedSequenceCommand { index, command }),
            Err(error) => issues.push(sequence_error_issue(index, error)),
        }
    }

    if issues.is_empty() {
        Ok(prepared)
    } else {
        Err(sequence_preflight_error(issues, kind))
    }
}

fn flattened_sequence_command(object: &serde_json::Map<String, Value>) -> Option<Value> {
    let args = object.get("args")?.as_object()?;
    let mut flattened = object.clone();
    flattened.remove("args");
    for (field, value) in args {
        if field == "purpose" {
            continue;
        }
        if flattened.contains_key(field) {
            return None;
        }
        flattened.insert(field.clone(), value.clone());
    }
    Some(Value::Object(flattened))
}

fn validate_sequence_command_count(count: usize, kind: SequenceKind) -> Result<(), CommandError> {
    if count == 0 {
        return Err(sequence_preflight_error(
            vec![sequence_invocation_issue(
                "sequence_commands_required",
                format!("{} requires at least one command", kind.tool_name()),
                "Add one or more command objects to commands and retry.",
                Some(RecoveryAction::AddFields {
                    fields: vec![RequiredField {
                        field: "commands[0]".to_owned(),
                        description: Some(
                            "Add a flat command object containing at least tool and op.".to_owned(),
                        ),
                    }],
                }),
            )],
            kind,
        ));
    }
    if count > SEQUENCE_MAX_COMMANDS {
        return Err(sequence_preflight_error(
            vec![sequence_invocation_issue(
                "sequence_commands_too_many",
                format!(
                    "{} accepts at most {SEQUENCE_MAX_COMMANDS} commands",
                    kind.tool_name()
                ),
                "Split the request into multiple sequence calls of at most 20 commands each.",
                Some(RecoveryAction::ChooseValue {
                    field: "commands.length".to_owned(),
                    choices: vec![json!(SEQUENCE_MAX_COMMANDS)],
                }),
            )],
            kind,
        ));
    }
    Ok(())
}

fn sequence_invocation_issue(
    code: &'static str,
    message: impl Into<String>,
    hint: &'static str,
    next_action: Option<RecoveryAction>,
) -> Value {
    json!({
        "path": "commands",
        "code": code,
        "message": message.into(),
        "hint": hint,
        "next_action": next_action,
    })
}

fn sequence_issue(
    index: usize,
    code: &'static str,
    message: impl Into<String>,
    hint: &'static str,
    next_action: Option<RecoveryAction>,
) -> Value {
    json!({
        "index": index,
        "path": format!("commands[{index}]"),
        "code": code,
        "message": message.into(),
        "hint": hint,
        "next_action": next_action,
    })
}

fn sequence_error_issue(index: usize, error: CommandError) -> Value {
    let message = error.message;
    let data = error.data.unwrap_or_else(|| json!({}));
    let mut next_action = data.get("next_action").cloned();
    if let Some(action) = next_action.as_mut() {
        prefix_sequence_action_fields(action, index);
    }
    json!({
        "index": index,
        "path": format!("commands[{index}]"),
        "code": data.get("code").and_then(Value::as_str).unwrap_or("invalid_input"),
        "message": message,
        "hint": data.get("hint"),
        "next_action": next_action,
    })
}

fn prefix_sequence_action_fields(action: &mut Value, index: usize) {
    match action.get("kind").and_then(Value::as_str) {
        Some("add_fields") => {
            if let Some(fields) = action.get_mut("fields").and_then(Value::as_array_mut) {
                for field in fields {
                    if let Some(name) = field.get_mut("field") {
                        prefix_sequence_field(name, index);
                    }
                }
            }
        }
        Some("remove_fields") => {
            if let Some(fields) = action.get_mut("fields").and_then(Value::as_array_mut) {
                for field in fields {
                    prefix_sequence_field(field, index);
                }
            }
        }
        Some("replace_field" | "choose_value") => {
            if let Some(field) = action.get_mut("field") {
                prefix_sequence_field(field, index);
            }
        }
        _ => {}
    }
}

fn prefix_sequence_field(field: &mut Value, index: usize) {
    if let Some(name) = field.as_str() {
        *field = Value::String(format!("commands[{index}].{name}"));
    }
}

fn sequence_preflight_error(issues: Vec<Value>, kind: SequenceKind) -> CommandError {
    let issue_count = issues.len();
    let mut data = RecoveryErrorData::actionable_input(
        "sequence_preflight_failed",
        format!(
            "Apply every nested error action and retry the same {} call. No command was executed.",
            kind.tool_name()
        ),
        RecoveryAction::ApplyErrorActions {
            errors_field: "errors".to_owned(),
        },
    );
    data.details.insert("ok".to_owned(), json!(false));
    data.details.insert("executed".to_owned(), json!(false));
    data.details.insert("completed".to_owned(), json!(0));
    data.details.insert("failed".to_owned(), json!(0));
    data.details.insert("skipped".to_owned(), json!(0));
    data.details
        .insert("results".to_owned(), Value::Array(Vec::new()));
    data.details
        .insert("errors".to_owned(), Value::Array(issues));
    CommandError::invalid_params(format!(
        "{} preflight found {issue_count} command input issue(s); nothing was executed",
        kind.tool_name()
    ))
    .with_data(data.into_value())
}

fn validate_sequence_command(
    command: &SequenceCommand,
    purpose: &str,
    kind: SequenceKind,
) -> Result<(), CommandError> {
    if !kind.allowed_tools().contains(&command.tool.as_str()) {
        return Err(actionable_input_error(
            "invalid_sequence_tool",
            format!("invalid tool for {}", kind.tool_name()),
            "Choose one of the tool values listed by next_action.choices.",
            RecoveryAction::ChooseValue {
                field: "tool".to_owned(),
                choices: kind
                    .allowed_tools()
                    .iter()
                    .map(|value| json!(value))
                    .collect(),
            },
        ));
    }

    if command.tool != "read" && command.direction.is_some() {
        return Err(actionable_input_error(
            "changes_fields_not_allowed",
            "direction is only valid for read op=changes",
            "Remove direction or use it only with a read changes command.",
            RecoveryAction::RemoveFields {
                fields: vec!["direction".to_owned()],
            },
        ));
    }

    match command.tool.as_str() {
        "read" => {
            executor::validate_read_operation(&command.clone().into_read_input(purpose.to_owned()))
        }
        "search" => executor::validate_search_operation(&search_input(command.clone(), purpose)?),
        "write" => {
            let input = write_input(command.clone(), purpose)?;
            executor::validate_write_operation(&input)?;
            executor::validate_static_write_content(&input)
        }
        "manage" => executor::validate_manage_operation(
            &command.clone().into_manage_input(purpose.to_owned()),
        ),
        _ => Err(invalid_input_error("invalid sequence tool")),
    }
}

fn search_input(
    command: SequenceCommand,
    purpose: &str,
) -> Result<notegate_command::SearchInput, CommandError> {
    Ok(notegate_command::SearchInput {
        purpose: purpose.to_owned(),
        op: command.op,
        target: required_input(command.target, "target", "search command")?,
        q: required_input(command.q, "q", "search command")?,
        kind: command.kind,
        match_mode: command.match_mode,
        lines: command.lines,
        include: command.include,
        exclude: command.exclude,
        limit: command.limit,
        cursor: command.cursor,
    })
}

fn write_input(
    command: SequenceCommand,
    purpose: &str,
) -> Result<notegate_command::WriteInput, CommandError> {
    Ok(notegate_command::WriteInput {
        purpose: purpose.to_owned(),
        op: command.op,
        target: required_input(command.target, "target", "write command")?,
        content: command.content,
        edits: command.edits,
        create: command.create,
        ensure_newline: command.ensure_newline,
        expected_sha256: command.expected_sha256,
    })
}

async fn execute_sequence_command(
    state: &AppState,
    context: &CommandContext,
    prepared: PreparedSequenceCommand,
    purpose: &str,
) -> SequenceOutcome {
    let tool = prepared.command.tool.clone();
    let op = prepared.command.op.clone();
    let result = dispatch_command(state, context, prepared.command, purpose).await;
    SequenceOutcome {
        index: prepared.index,
        tool,
        op,
        result,
    }
}

fn sequence_response(outcomes: Vec<SequenceOutcome>, skipped: usize) -> Value {
    let mut completed = 0_usize;
    let mut failed = 0_usize;
    let results = outcomes
        .into_iter()
        .map(|outcome| match outcome.result {
            Ok(value) => {
                completed = completed.saturating_add(1);
                json!({
                    "index": outcome.index,
                    "tool": outcome.tool,
                    "op": outcome.op,
                    "ok": true,
                    "result": value,
                })
            }
            Err(error) => {
                failed = failed.saturating_add(1);
                let mut error = command_error_json(error);
                if let Some(action) = error.pointer_mut("/data/next_action") {
                    prefix_sequence_action_fields(action, outcome.index);
                }
                json!({
                    "index": outcome.index,
                    "tool": outcome.tool,
                    "op": outcome.op,
                    "ok": false,
                    "error": error,
                })
            }
        })
        .collect::<Vec<_>>();

    json!({
        "ok": failed == 0,
        "completed": completed,
        "failed": failed,
        "skipped": skipped,
        "results": results,
    })
}

fn command_error_json(error: CommandError) -> Value {
    let code = match error.class {
        CommandErrorClass::InvalidParams => -32602,
        CommandErrorClass::InvalidRequest => -32600,
        CommandErrorClass::TemporaryUnavailable => -32001,
        CommandErrorClass::CapacityBusy => -32002,
        CommandErrorClass::Internal => -32603,
    };
    json!({
        "code": code,
        "message": error.message,
        "data": error.data,
    })
}

async fn dispatch_command(
    state: &AppState,
    context: &CommandContext,
    command: SequenceCommand,
    purpose: &str,
) -> Result<Value, CommandError> {
    match command.tool.as_str() {
        "read" => executor::read(state, context, command.into_read_input(purpose.to_owned())).await,
        "search" => executor::search(state, context, search_input(command, purpose)?).await,
        "write" => executor::write(state, context, write_input(command, purpose)?).await,
        "manage" => {
            executor::manage(
                state,
                context,
                command.into_manage_input(purpose.to_owned()),
            )
            .await
        }
        _ => Err(invalid_input_error("invalid sequence tool")),
    }
}

#[cfg(test)]
mod tests;
