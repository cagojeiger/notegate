use super::*;

use crate::mcp::contract::{McpErrorData, error_json};

mod read;
mod write;

pub use read::{RunReadSequenceInput, run_read_sequence};
pub use write::{RunWriteSequenceInput, run_write_sequence};

const SEQUENCE_MAX_COMMANDS: usize = 20;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SequenceCommand {
    tool: String,
    op: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    destination: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default, rename = "match")]
    match_mode: Option<String>,
    #[serde(default)]
    lines: Option<String>,
    #[serde(default)]
    include: Option<Vec<String>>,
    #[serde(default)]
    exclude: Option<Vec<String>>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    edits: Option<Vec<Value>>,
    #[serde(default)]
    create: bool,
    #[serde(default)]
    parents: bool,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    ensure_newline: bool,
    #[serde(default)]
    depth: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    start_line: Option<i64>,
    #[serde(default)]
    max_lines: Option<i64>,
    #[serde(default)]
    max_bytes: Option<usize>,
    #[serde(default)]
    expected_sha256: Option<String>,
    #[serde(default)]
    if_none_match_sha256: Option<String>,
}

impl SequenceCommand {
    fn into_read_input(self, purpose: String) -> ReadInput {
        ReadInput {
            purpose,
            op: self.op,
            target: self.target,
            name: self.name,
            depth: self.depth,
            limit: self.limit,
            cursor: self.cursor,
            direction: self.direction,
            start_line: self.start_line,
            max_lines: self.max_lines,
            max_bytes: self.max_bytes,
            if_none_match_sha256: self.if_none_match_sha256,
        }
    }

    fn into_search_input(self, purpose: String) -> Result<SearchInput, ErrorData> {
        Ok(SearchInput {
            purpose,
            op: self.op,
            target: required(self.target, "target", "search command")?,
            q: required(self.q, "q", "search command")?,
            kind: self.kind,
            match_mode: self.match_mode,
            lines: self.lines,
            include: self.include,
            exclude: self.exclude,
            limit: self.limit,
            cursor: self.cursor,
        })
    }

    fn into_write_input(self, purpose: String) -> Result<WriteInput, ErrorData> {
        Ok(WriteInput {
            purpose,
            op: self.op,
            target: required(self.target, "target", "write command")?,
            content: self.content,
            edits: self.edits,
            create: self.create,
            ensure_newline: self.ensure_newline,
            expected_sha256: self.expected_sha256,
        })
    }

    fn into_manage_input(self, purpose: String) -> ManageInput {
        ManageInput {
            purpose,
            op: self.op,
            target: self.target,
            source: self.source,
            destination: self.destination,
            parents: self.parents,
            recursive: self.recursive,
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceKind {
    Read,
    Write,
}

impl SequenceKind {
    fn tool_name(self) -> &'static str {
        match self {
            Self::Read => "run_read_sequence",
            Self::Write => "run_write_sequence",
        }
    }

    fn allowed_tools(self) -> &'static [&'static str] {
        match self {
            Self::Read => &["read", "search"],
            Self::Write => &["write", "manage"],
        }
    }

    fn operation_help(self) -> &'static str {
        match self {
            Self::Read => "read=spaces/ls/tree/stat/read/changes; search=find/grep.",
            Self::Write => "write=write/append/patch/edit; manage=mkdir/mv/cp/rm.",
        }
    }
}

#[derive(Debug, Clone)]
struct PreparedSequenceCommand {
    index: usize,
    command: SequenceCommand,
}

struct SequenceOutcome {
    index: usize,
    tool: String,
    op: String,
    result: Result<Json<Value>, ErrorData>,
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
) -> Result<Vec<PreparedSequenceCommand>, ErrorData> {
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
                Some(McpAction::RemoveFields {
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
                replacement.map(|value| McpAction::ReplaceField {
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
                Some(McpAction::RemoveFields {
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
        .map(|(field, description)| crate::mcp::contract::RequiredField {
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
                Some(McpAction::AddFields {
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
                    Some(McpAction::RemoveFields {
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

fn validate_sequence_command_count(count: usize, kind: SequenceKind) -> Result<(), ErrorData> {
    if count == 0 {
        return Err(sequence_preflight_error(
            vec![sequence_invocation_issue(
                "sequence_commands_required",
                format!("{} requires at least one command", kind.tool_name()),
                "Add one or more command objects to commands and retry.",
                Some(McpAction::AddFields {
                    fields: vec![crate::mcp::contract::RequiredField {
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
                Some(McpAction::ChooseValue {
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
    next_action: Option<McpAction>,
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
    next_action: Option<McpAction>,
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

fn sequence_error_issue(index: usize, error: ErrorData) -> Value {
    let error = error_json(error);
    let mut next_action = error.pointer("/data/next_action").cloned();
    if let Some(action) = next_action.as_mut() {
        prefix_sequence_action_fields(action, index);
    }
    json!({
        "index": index,
        "path": format!("commands[{index}]"),
        "code": error.pointer("/data/code").and_then(Value::as_str).unwrap_or("invalid_input"),
        "message": error.get("message").and_then(Value::as_str).unwrap_or("invalid command"),
        "hint": error.pointer("/data/hint"),
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

fn sequence_preflight_error(issues: Vec<Value>, kind: SequenceKind) -> ErrorData {
    let issue_count = issues.len();
    let mut data = McpErrorData::actionable_input(
        "sequence_preflight_failed",
        format!(
            "Apply every nested error action and retry the same {} call. No command was executed.",
            kind.tool_name()
        ),
        McpAction::ApplyErrorActions {
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
    ErrorData::invalid_params(
        format!(
            "{} preflight found {issue_count} command input issue(s); nothing was executed",
            kind.tool_name()
        ),
        Some(data.into_value()),
    )
}

fn validate_sequence_command(
    command: &SequenceCommand,
    purpose: &str,
    kind: SequenceKind,
) -> Result<(), ErrorData> {
    if !kind.allowed_tools().contains(&command.tool.as_str()) {
        return Err(actionable_input_error(
            "invalid_sequence_tool",
            format!("invalid tool for {}", kind.tool_name()),
            "Choose one of the tool values listed by next_action.choices.",
            McpAction::ChooseValue {
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
            McpAction::RemoveFields {
                fields: vec!["direction".to_owned()],
            },
        ));
    }

    match command.tool.as_str() {
        "read" => validate_read_operation(&command.clone().into_read_input(purpose.to_owned())),
        "search" => {
            validate_search_operation(&command.clone().into_search_input(purpose.to_owned())?)
        }
        "write" => {
            let input = command.clone().into_write_input(purpose.to_owned())?;
            validate_write_operation(&input)?;
            validate_static_write_content(&input)
        }
        "manage" => {
            validate_manage_operation(&command.clone().into_manage_input(purpose.to_owned()))
        }
        _ => Err(invalid_input_error("invalid sequence tool")),
    }
}

async fn execute_sequence_command(
    state: &AppState,
    parts: &Parts,
    prepared: PreparedSequenceCommand,
    purpose: &str,
) -> SequenceOutcome {
    let tool = prepared.command.tool.clone();
    let op = prepared.command.op.clone();
    let result = dispatch_command(state, parts, prepared.command, purpose).await;
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
            Ok(Json(value)) => {
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
                let mut error = error_json(error);
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

async fn dispatch_command(
    state: &AppState,
    parts: &Parts,
    command: SequenceCommand,
    purpose: &str,
) -> Result<Json<Value>, ErrorData> {
    match command.tool.as_str() {
        "read" => {
            read(
                state,
                parts,
                Parameters(command.into_read_input(purpose.to_owned())),
            )
            .await
        }
        "search" => {
            search(
                state,
                parts,
                Parameters(command.into_search_input(purpose.to_owned())?),
            )
            .await
        }
        "write" => {
            write(
                state,
                parts,
                Parameters(command.into_write_input(purpose.to_owned())?),
            )
            .await
        }
        "manage" => {
            manage(
                state,
                parts,
                Parameters(command.into_manage_input(purpose.to_owned())),
            )
            .await
        }
        _ => Err(invalid_input_error("invalid sequence tool")),
    }
}

#[cfg(test)]
mod tests;
