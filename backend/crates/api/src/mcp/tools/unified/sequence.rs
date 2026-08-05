use super::*;

use futures_util::{StreamExt, stream};

use crate::mcp::contract::{McpErrorData, error_json};

const RUN_SEQUENCE_MAX_COMMANDS: usize = 20;
const RUN_SEQUENCE_READ_CONCURRENCY: usize = 4;

/// Public schema for one run-sequence command. Runtime parsing deliberately stays on the
/// permissive raw-object path so preflight can aggregate every command error before execution.
#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "tool", rename_all = "snake_case")]
#[schemars(inline)]
enum SequenceCommandSchema {
    Read(SequenceReadCommandSchema),
    Search(SequenceSearchCommandSchema),
    Write(SequenceWriteCommandSchema),
    Manage(SequenceManageCommandSchema),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
enum SequenceReadOperationSchema {
    Spaces,
    Ls,
    Tree,
    Stat,
    Read,
    Changes,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
enum SequenceSearchOperationSchema {
    Find,
    Grep,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
enum SequenceWriteOperationSchema {
    Write,
    Append,
    Patch,
    Edit,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
enum SequenceManageOperationSchema {
    Mkdir,
    Mv,
    Cp,
    Rm,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceReadCommandSchema {
    /// Read operation: spaces/ls/tree/stat/read/changes.
    op: SequenceReadOperationSchema,
    /// Target in `<space>:/absolute/path` form when required by the operation.
    target: Option<String>,
    /// Optional exact, case-sensitive space name filter for spaces.
    name: Option<String>,
    /// Tree depth for tree.
    depth: Option<i64>,
    /// Page size.
    limit: Option<i64>,
    /// Opaque pagination cursor.
    cursor: Option<String>,
    /// Changes direction: older/newer.
    direction: Option<String>,
    /// 1-based first line for read.
    start_line: Option<i64>,
    /// Maximum lines for read.
    max_lines: Option<i64>,
    /// Maximum bytes for read.
    max_bytes: Option<usize>,
    /// Conditional read guard.
    if_none_match_sha256: Option<String>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceSearchCommandSchema {
    /// Search operation: find/grep.
    op: SequenceSearchOperationSchema,
    /// Scope target in `<space>:/absolute/path` form.
    target: String,
    /// Search query.
    q: String,
    /// Find node kind filter: folder/text/file.
    kind: Option<String>,
    /// Find or grep match mode.
    #[serde(rename = "match")]
    match_mode: Option<String>,
    /// Grep line detail: none/first/all.
    lines: Option<String>,
    /// Optional path glob includes.
    include: Option<Vec<String>>,
    /// Optional path glob excludes.
    exclude: Option<Vec<String>>,
    /// Page size.
    limit: Option<i64>,
    /// Opaque pagination cursor.
    cursor: Option<String>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceWriteCommandSchema {
    /// Write operation: write/append/patch/edit.
    op: SequenceWriteOperationSchema,
    /// Text target in `<space>:/absolute/path` form.
    target: String,
    /// Text content for write/append.
    content: Option<String>,
    /// Patch or line-edit entries for patch/edit.
    #[schemars(with = "Option<Vec<WriteEditEntrySchema>>")]
    edits: Option<Vec<Value>>,
    /// Create missing text for write/append.
    #[serde(default)]
    create: bool,
    /// Insert a newline before appended content when needed.
    #[serde(default)]
    ensure_newline: bool,
    /// Node revision from the latest read.
    expected_revision: Option<i64>,
    /// Optional content-specific optimistic guard.
    expected_sha256: Option<String>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceManageCommandSchema {
    /// Manage operation: mkdir/mv/cp/rm.
    op: SequenceManageOperationSchema,
    /// Target for mkdir/rm.
    target: Option<String>,
    /// Source target for mv/cp.
    source: Option<String>,
    /// Destination target for mv/cp.
    destination: Option<String>,
    /// Create missing parent folders for mkdir.
    #[serde(default)]
    parents: bool,
    /// Required for folder cp/rm.
    #[serde(default)]
    recursive: bool,
    /// Node revision from the latest read.
    expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunSequenceInput {
    /// Reason for this MCP invocation. Required once at the top level; commands inherit it and must not include purpose; maximum 200 characters.
    pub purpose: String,
    /// Ordered flat command objects. Each includes tool and op, omits purpose and args; 1..20.
    #[schemars(with = "Vec<SequenceCommandSchema>", length(min = 1, max = 20))]
    pub commands: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceCommand {
    /// Tool category for this command: read/search/write/manage.
    pub tool: String,

    /// Operation for the selected tool: read=spaces/ls/tree/stat/read/changes, search=find/grep, write=write/append/patch/edit, manage=mkdir/mv/cp/rm.
    pub op: String,

    /// Single target in `<space>:/absolute/path` form. The space name segment is exact and case-sensitive.
    #[serde(default)]
    pub target: Option<String>,
    /// Source target for `mv` and `cp`.
    #[serde(default)]
    pub source: Option<String>,
    /// Destination target for `mv` and `cp`.
    #[serde(default)]
    pub destination: Option<String>,

    /// Optional exact, case-sensitive space name filter for `read op=spaces`.
    #[serde(default)]
    pub name: Option<String>,
    /// Search query for `find` and `grep`. Matching inside the resolved space is case-insensitive.
    #[serde(default)]
    pub q: Option<String>,
    /// Node kind filter: `folder`, `text`, or `file`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Match mode. `find`: contains/regex/glob. `grep`: literal/regex. All modes are case-insensitive.
    #[serde(default, rename = "match")]
    pub match_mode: Option<String>,
    /// Grep line detail: none/first/all.
    #[serde(default)]
    pub lines: Option<String>,
    /// Optional path glob includes.
    #[serde(default)]
    pub include: Option<Vec<String>>,
    /// Optional path glob excludes.
    #[serde(default)]
    pub exclude: Option<Vec<String>>,

    /// Text content for write/append.
    #[serde(default)]
    pub content: Option<String>,
    /// Patch or line-edit entries for patch/edit.
    #[serde(default)]
    pub edits: Option<Vec<Value>>,

    /// Create missing text for write/append.
    #[serde(default)]
    pub create: bool,
    /// Create missing parent folders for mkdir.
    #[serde(default)]
    pub parents: bool,
    /// Required for folder cp/rm.
    #[serde(default)]
    pub recursive: bool,
    /// Insert a newline before appended content when needed.
    #[serde(default)]
    pub ensure_newline: bool,

    /// Tree/list depth.
    #[serde(default)]
    pub depth: Option<i64>,
    /// Page size.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor for paginated reads and searches.
    #[serde(default)]
    pub cursor: Option<String>,
    /// For `read op=changes`, read older events (default) or newer events.
    #[serde(default)]
    pub direction: Option<String>,

    /// 1-based first line for read.
    #[serde(default)]
    pub start_line: Option<i64>,
    /// Maximum lines for read.
    #[serde(default)]
    pub max_lines: Option<i64>,
    /// Maximum bytes for read.
    #[serde(default)]
    pub max_bytes: Option<usize>,

    /// Node revision from the latest read.
    #[serde(default)]
    pub expected_revision: Option<i64>,
    /// Optional content-specific optimistic guard.
    #[serde(default)]
    pub expected_sha256: Option<String>,
    /// Conditional read guard.
    #[serde(default)]
    pub if_none_match_sha256: Option<String>,
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
            expected_revision: self.expected_revision,
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
            expected_revision: self.expected_revision,
        }
    }
}

const SEQUENCE_READ_COMMAND_FIELDS: &[&str] = &[
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

const SEQUENCE_SEARCH_COMMAND_FIELDS: &[&str] = &[
    "tool", "op", "target", "q", "kind", "match", "lines", "include", "exclude", "limit", "cursor",
];

const SEQUENCE_WRITE_COMMAND_FIELDS: &[&str] = &[
    "tool",
    "op",
    "target",
    "content",
    "edits",
    "create",
    "ensure_newline",
    "expected_revision",
    "expected_sha256",
];

const SEQUENCE_MANAGE_COMMAND_FIELDS: &[&str] = &[
    "tool",
    "op",
    "target",
    "source",
    "destination",
    "parents",
    "recursive",
    "expected_revision",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceAccessMode {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceAccessScope {
    Exact,
    Subtree,
    Space,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SequenceAccess {
    mode: SequenceAccessMode,
    scope: SequenceAccessScope,
    space: String,
    path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceExecutionClass {
    PureRead,
    WideRead,
    ConsistencyRead,
    PointMutation,
    NamespaceMutation,
    StructuralBarrier,
}

impl SequenceExecutionClass {
    fn is_read_only(self) -> bool {
        matches!(
            self,
            Self::PureRead | Self::WideRead | Self::ConsistencyRead
        )
    }
}

struct SequenceCommandPlan {
    execution_class: SequenceExecutionClass,
    accesses: Vec<SequenceAccess>,
}

fn sequence_tool_fields(tool: &str) -> Option<&'static [&'static str]> {
    match tool {
        "read" => Some(SEQUENCE_READ_COMMAND_FIELDS),
        "search" => Some(SEQUENCE_SEARCH_COMMAND_FIELDS),
        "write" => Some(SEQUENCE_WRITE_COMMAND_FIELDS),
        "manage" => Some(SEQUENCE_MANAGE_COMMAND_FIELDS),
        _ => None,
    }
}

fn is_sequence_command_field(field: &str) -> bool {
    [
        SEQUENCE_READ_COMMAND_FIELDS,
        SEQUENCE_SEARCH_COMMAND_FIELDS,
        SEQUENCE_WRITE_COMMAND_FIELDS,
        SEQUENCE_MANAGE_COMMAND_FIELDS,
    ]
    .into_iter()
    .any(|fields| fields.contains(&field))
}

#[derive(Debug, Clone)]
struct PreparedSequenceCommand {
    index: usize,
    command: SequenceCommand,
    execution_class: SequenceExecutionClass,
    accesses: Vec<SequenceAccess>,
}

impl PreparedSequenceCommand {
    fn is_read_only(&self) -> bool {
        self.execution_class.is_read_only()
    }

    fn is_structural_barrier(&self) -> bool {
        self.execution_class == SequenceExecutionClass::StructuralBarrier
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SequenceDependencyGraph {
    dependencies: Vec<Vec<usize>>,
}

impl SequenceDependencyGraph {
    fn depends_on(&self, command_index: usize, dependency_index: usize) -> bool {
        self.dependencies
            .get(command_index)
            .is_some_and(|dependencies| dependencies.contains(&dependency_index))
    }
}

fn prepare_sequence_commands(
    commands: Vec<Value>,
    purpose: &str,
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
                "purpose belongs to the run_sequence invocation, not an internal command",
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
                "run_sequence commands are flat objects and do not use an args wrapper",
                "Move every field from args into this command object, then remove args.",
                replacement.map(|value| McpAction::ReplaceField {
                    field: format!("commands[{index}]"),
                    value,
                }),
            ));
        }

        let unknown_fields = candidate
            .keys()
            .filter(|field| !is_sequence_command_field(field))
            .cloned()
            .collect::<Vec<_>>();
        if !unknown_fields.is_empty() {
            issues.push(sequence_issue(
                index,
                "sequence_command_unknown_fields",
                "run_sequence command contains unsupported fields",
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

        let missing_fields = [
            ("tool", "One of: read, search, write, manage."),
            (
                "op",
                "Allowed values depend on tool: read=spaces/ls/tree/stat/read/changes; search=find/grep; write=write/append/patch/edit; manage=mkdir/mv/cp/rm.",
            ),
        ]
            .into_iter()
            .filter(|(field, _)| !candidate.contains_key(*field))
            .map(|(field, description)| crate::mcp::contract::RequiredField {
                field: format!("commands[{index}].{field}"),
                description: Some(description.to_owned()),
            })
            .collect::<Vec<_>>();
        if !missing_fields.is_empty() {
            issues.push(sequence_issue(
                index,
                "sequence_command_required_fields_missing",
                "run_sequence command is missing required fields",
                "Add every field listed by next_action.fields and retry.",
                Some(McpAction::AddFields {
                    fields: missing_fields,
                }),
            ));
            shape_blocked = true;
        }

        if let Some(tool) = candidate.get("tool").and_then(Value::as_str)
            && let Some(allowed_fields) = sequence_tool_fields(tool)
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
                    format!("run_sequence {tool} command contains fields for another tool"),
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
                    format!("invalid run_sequence command: {error}"),
                    "Correct the field type or value at the reported command index and retry.",
                    None,
                ));
                continue;
            }
        };

        match validate_and_describe_sequence_command(&command, purpose) {
            Ok(plan) => prepared.push(PreparedSequenceCommand {
                index,
                command,
                execution_class: plan.execution_class,
                accesses: plan.accesses,
            }),
            Err(error) => issues.push(sequence_error_issue(index, error)),
        }
    }

    if issues.is_empty() {
        Ok(prepared)
    } else {
        Err(sequence_preflight_error(issues))
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

fn validate_sequence_command_count(count: usize) -> Result<(), ErrorData> {
    if count == 0 {
        return Err(sequence_preflight_error(vec![sequence_invocation_issue(
            "sequence_commands_required",
            "run_sequence requires at least one command",
            "Add one or more command objects to commands and retry.",
            Some(McpAction::AddFields {
                fields: vec![crate::mcp::contract::RequiredField {
                    field: "commands[0]".to_owned(),
                    description: Some(
                        "Add a flat command object containing at least tool and op.".to_owned(),
                    ),
                }],
            }),
        )]));
    }
    if count > RUN_SEQUENCE_MAX_COMMANDS {
        return Err(sequence_preflight_error(vec![sequence_invocation_issue(
            "sequence_commands_too_many",
            format!("run_sequence accepts at most {RUN_SEQUENCE_MAX_COMMANDS} commands"),
            "Split the request into multiple run_sequence calls of at most 20 commands each.",
            Some(McpAction::ChooseValue {
                field: "commands.length".to_owned(),
                choices: vec![json!(RUN_SEQUENCE_MAX_COMMANDS)],
            }),
        )]));
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

fn sequence_preflight_error(issues: Vec<Value>) -> ErrorData {
    let issue_count = issues.len();
    let mut data = McpErrorData::actionable_input(
        "sequence_preflight_failed",
        "Apply every nested error action and retry the same run_sequence call. No command was executed.",
        McpAction::ApplyErrorActions {
            errors_field: "errors".to_owned(),
        },
    );
    data.details.insert("ok".to_owned(), json!(false));
    data.details.insert("phase".to_owned(), json!("preflight"));
    data.details.insert("executed".to_owned(), json!(false));
    data.details.insert("completed".to_owned(), json!(0));
    data.details.insert("failed_index".to_owned(), Value::Null);
    data.details
        .insert("results".to_owned(), Value::Array(Vec::new()));
    data.details
        .insert("errors".to_owned(), Value::Array(issues));
    ErrorData::invalid_params(
        format!(
            "run_sequence preflight found {} command input issue(s); nothing was executed",
            issue_count
        ),
        Some(data.into_value()),
    )
}

fn validate_and_describe_sequence_command(
    command: &SequenceCommand,
    purpose: &str,
) -> Result<SequenceCommandPlan, ErrorData> {
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
        "read" => plan_sequence_read(command, purpose),
        "search" => plan_sequence_search(command, purpose),
        "write" => plan_sequence_write(command, purpose),
        "manage" => plan_sequence_manage(command, purpose),
        _ => Err(actionable_input_error(
            "invalid_sequence_tool",
            "invalid tool for run_sequence",
            "Choose one of the tool values listed by next_action.choices.",
            McpAction::ChooseValue {
                field: "tool".to_owned(),
                choices: ["read", "search", "write", "manage"]
                    .into_iter()
                    .map(|value| json!(value))
                    .collect(),
            },
        )),
    }
}

fn plan_sequence_read(
    command: &SequenceCommand,
    purpose: &str,
) -> Result<SequenceCommandPlan, ErrorData> {
    let input = command.clone().into_read_input(purpose.to_owned());
    validate_read_operation(&input)?;
    match input.op.as_str() {
        "spaces" => Ok(sequence_command_plan(
            SequenceExecutionClass::PureRead,
            Vec::new(),
        )),
        "ls" | "tree" => Ok(sequence_command_plan(
            SequenceExecutionClass::WideRead,
            vec![sequence_access(
                required_ref(input.target.as_ref(), "target", input.op.as_str())?,
                SequenceAccessMode::Read,
                SequenceAccessScope::Subtree,
            )?],
        )),
        "stat" => Ok(sequence_command_plan(
            SequenceExecutionClass::PureRead,
            vec![sequence_access(
                required_ref(input.target.as_ref(), "target", "stat")?,
                SequenceAccessMode::Read,
                SequenceAccessScope::Subtree,
            )?],
        )),
        "read" => Ok(sequence_command_plan(
            SequenceExecutionClass::PureRead,
            vec![sequence_access(
                required_ref(input.target.as_ref(), "target", "read")?,
                SequenceAccessMode::Read,
                SequenceAccessScope::Exact,
            )?],
        )),
        "changes" => Ok(sequence_command_plan(
            SequenceExecutionClass::ConsistencyRead,
            vec![sequence_access(
                required_ref(input.target.as_ref(), "target", "changes")?,
                SequenceAccessMode::Read,
                SequenceAccessScope::Space,
            )?],
        )),
        _ => Err(invalid_op(
            "read",
            &["spaces", "ls", "tree", "stat", "read", "changes"],
        )),
    }
}

fn plan_sequence_search(
    command: &SequenceCommand,
    purpose: &str,
) -> Result<SequenceCommandPlan, ErrorData> {
    let input = command.clone().into_search_input(purpose.to_owned())?;
    validate_search_operation(&input)?;
    Ok(sequence_command_plan(
        SequenceExecutionClass::WideRead,
        vec![sequence_access(
            &input.target,
            SequenceAccessMode::Read,
            SequenceAccessScope::Subtree,
        )?],
    ))
}

fn plan_sequence_write(
    command: &SequenceCommand,
    purpose: &str,
) -> Result<SequenceCommandPlan, ErrorData> {
    let input = command.clone().into_write_input(purpose.to_owned())?;
    validate_write_operation(&input)?;
    validate_static_write_content(&input)?;
    let execution_class = if matches!(input.op.as_str(), "write" | "append") && input.create {
        SequenceExecutionClass::NamespaceMutation
    } else {
        SequenceExecutionClass::PointMutation
    };
    Ok(sequence_command_plan(
        execution_class,
        vec![sequence_access(
            &input.target,
            SequenceAccessMode::Write,
            SequenceAccessScope::Exact,
        )?],
    ))
}

fn plan_sequence_manage(
    command: &SequenceCommand,
    purpose: &str,
) -> Result<SequenceCommandPlan, ErrorData> {
    let input = command.clone().into_manage_input(purpose.to_owned());
    validate_manage_operation(&input)?;
    match input.op.as_str() {
        "mkdir" => Ok(sequence_command_plan(
            if input.parents {
                SequenceExecutionClass::StructuralBarrier
            } else {
                SequenceExecutionClass::NamespaceMutation
            },
            vec![sequence_access(
                required_ref(input.target.as_ref(), "target", "mkdir")?,
                SequenceAccessMode::Write,
                SequenceAccessScope::Exact,
            )?],
        )),
        "rm" => Ok(sequence_command_plan(
            SequenceExecutionClass::StructuralBarrier,
            vec![sequence_access(
                required_ref(input.target.as_ref(), "target", "rm")?,
                SequenceAccessMode::Write,
                SequenceAccessScope::Subtree,
            )?],
        )),
        "mv" => Ok(sequence_command_plan(
            SequenceExecutionClass::StructuralBarrier,
            vec![
                sequence_access(
                    required_ref(input.source.as_ref(), "source", "mv")?,
                    SequenceAccessMode::Write,
                    SequenceAccessScope::Subtree,
                )?,
                sequence_access(
                    required_ref(input.destination.as_ref(), "destination", "mv")?,
                    SequenceAccessMode::Write,
                    SequenceAccessScope::Subtree,
                )?,
            ],
        )),
        "cp" => Ok(sequence_command_plan(
            SequenceExecutionClass::StructuralBarrier,
            vec![
                sequence_access(
                    required_ref(input.source.as_ref(), "source", "cp")?,
                    SequenceAccessMode::Read,
                    SequenceAccessScope::Subtree,
                )?,
                sequence_access(
                    required_ref(input.destination.as_ref(), "destination", "cp")?,
                    SequenceAccessMode::Write,
                    SequenceAccessScope::Subtree,
                )?,
            ],
        )),
        _ => Err(invalid_op("manage", &["mkdir", "mv", "cp", "rm"])),
    }
}

fn sequence_command_plan(
    execution_class: SequenceExecutionClass,
    accesses: Vec<SequenceAccess>,
) -> SequenceCommandPlan {
    SequenceCommandPlan {
        execution_class,
        accesses,
    }
}

fn sequence_access(
    target: &str,
    mode: SequenceAccessMode,
    scope: SequenceAccessScope,
) -> Result<SequenceAccess, ErrorData> {
    let target = parse_input_target(target)?;
    Ok(SequenceAccess {
        mode,
        scope,
        space: target.space,
        path: (scope != SequenceAccessScope::Space).then_some(target.path),
    })
}

fn sequence_commands_conflict(
    left: &PreparedSequenceCommand,
    right: &PreparedSequenceCommand,
) -> bool {
    left.accesses.iter().any(|left| {
        right
            .accesses
            .iter()
            .any(|right| sequence_accesses_conflict(left, right))
    })
}

fn build_sequence_dependency_graph(
    commands: &[PreparedSequenceCommand],
) -> SequenceDependencyGraph {
    let mut dependencies = vec![Vec::new(); commands.len()];
    for (earlier_position, earlier) in commands.iter().enumerate() {
        for (later, later_dependencies) in commands
            .iter()
            .skip(earlier_position + 1)
            .zip(dependencies.iter_mut().skip(earlier_position + 1))
        {
            if sequence_requires_dependency(earlier, later) {
                later_dependencies.push(earlier.index);
            }
        }
    }
    SequenceDependencyGraph { dependencies }
}

fn sequence_requires_dependency(
    earlier: &PreparedSequenceCommand,
    later: &PreparedSequenceCommand,
) -> bool {
    earlier.is_structural_barrier()
        || later.is_structural_barrier()
        || (!earlier.is_read_only() && !later.is_read_only())
        || (earlier.is_read_only() && !later.is_read_only())
        || sequence_commands_conflict(earlier, later)
}

fn sequence_accesses_conflict(left: &SequenceAccess, right: &SequenceAccess) -> bool {
    if left.space != right.space
        || (left.mode == SequenceAccessMode::Read && right.mode == SequenceAccessMode::Read)
    {
        return false;
    }
    if left.scope == SequenceAccessScope::Space || right.scope == SequenceAccessScope::Space {
        return true;
    }

    let Some(left_path) = left.path.as_deref() else {
        return true;
    };
    let Some(right_path) = right.path.as_deref() else {
        return true;
    };
    match (left.scope, right.scope) {
        (SequenceAccessScope::Exact, SequenceAccessScope::Exact) => left_path == right_path,
        (SequenceAccessScope::Subtree, SequenceAccessScope::Exact) => {
            path_contains(left_path, right_path)
        }
        (SequenceAccessScope::Exact, SequenceAccessScope::Subtree) => {
            path_contains(right_path, left_path)
        }
        (SequenceAccessScope::Subtree, SequenceAccessScope::Subtree) => {
            path_contains(left_path, right_path) || path_contains(right_path, left_path)
        }
        (SequenceAccessScope::Space, _) | (_, SequenceAccessScope::Space) => true,
    }
}

fn path_contains(root: &str, candidate: &str) -> bool {
    root == candidate
        || root == "/"
        || candidate
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub async fn run_sequence(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<RunSequenceInput>,
) -> Result<Json<Value>, ErrorData> {
    validate_sequence_command_count(input.commands.len())?;
    let command_count = input.commands.len();
    let commands = prepare_sequence_commands(input.commands, &input.purpose)?;
    let graph = build_sequence_dependency_graph(&commands);
    let mut results = Vec::with_capacity(command_count);
    let mut cursor = 0;
    // A read-only prefix must finish before the next mutation so an earlier read failure cannot
    // leave a later write committed. Once a mutation is first in the remaining sequence, later
    // non-conflicting reads may overlap it because discarding those read results has no side effect.
    // Mutations never overlap each other, preserving the public fail-fast ordering contract.
    while cursor < commands.len() {
        let current = commands.get(cursor).ok_or_else(sequence_scheduler_error)?;
        if current.is_read_only() {
            let end = next_mutation_index(&commands, cursor);
            let batch = commands
                .get(cursor..end)
                .ok_or_else(sequence_scheduler_error)?;
            let outcomes =
                execute_read_only_commands(state, parts, batch.to_vec(), &input.purpose).await;
            if let Some(response) = append_sequence_outcomes(&mut results, outcomes) {
                return Ok(Json(response));
            }
            cursor = end;
            continue;
        }

        let mutation = current.clone();
        let end = next_mutation_index(&commands, cursor + 1);
        let following_reads = commands
            .get(cursor + 1..end)
            .ok_or_else(sequence_scheduler_error)?;
        let (parallel_reads, dependent_reads): (Vec<_>, Vec<_>) = following_reads
            .iter()
            .cloned()
            .partition(|command| !graph.depends_on(command.index, mutation.index));

        let mutation_future = execute_sequence_command(state, parts, mutation, &input.purpose);
        let parallel_read_future =
            execute_read_only_commands(state, parts, parallel_reads, &input.purpose);
        let (mutation_outcome, mut read_outcomes) =
            tokio::join!(mutation_future, parallel_read_future);

        if let Some(response) = append_sequence_outcomes(&mut results, vec![mutation_outcome]) {
            return Ok(Json(response));
        }

        let parallel_failure_index = read_outcomes
            .iter()
            .find(|outcome| outcome.result.is_err())
            .map(|outcome| outcome.index);
        let dependent_reads = dependent_reads
            .into_iter()
            .take_while(|command| {
                parallel_failure_index.is_none_or(|failed_index| command.index < failed_index)
            })
            .collect();
        read_outcomes.extend(
            execute_read_only_commands(state, parts, dependent_reads, &input.purpose).await,
        );
        read_outcomes.sort_by_key(|outcome| outcome.index);
        if let Some(response) = append_sequence_outcomes(&mut results, read_outcomes) {
            return Ok(Json(response));
        }
        cursor = end;
    }

    Ok(Json(json!({
        "ok": true,
        "phase": "complete",
        "executed": true,
        "completed": results.len(),
        "failed_index": null,
        "results": results,
    })))
}

fn sequence_scheduler_error() -> ErrorData {
    ErrorData::internal_error("run_sequence scheduler produced an invalid range", None)
}

struct SequenceOutcome {
    index: usize,
    tool: String,
    op: String,
    result: Result<Json<Value>, ErrorData>,
}

fn next_mutation_index(commands: &[PreparedSequenceCommand], start: usize) -> usize {
    commands
        .get(start..)
        .unwrap_or_default()
        .iter()
        .position(|command| !command.is_read_only())
        .map_or(commands.len(), |offset| start + offset)
}

async fn execute_read_only_commands(
    state: &AppState,
    parts: &Parts,
    commands: Vec<PreparedSequenceCommand>,
    purpose: &str,
) -> Vec<SequenceOutcome> {
    let mut pending = stream::iter(commands)
        .map(|command| execute_sequence_command(state, parts, command, purpose))
        .buffered(RUN_SEQUENCE_READ_CONCURRENCY);
    let mut outcomes = Vec::new();
    while let Some(outcome) = pending.next().await {
        let failed = outcome.result.is_err();
        outcomes.push(outcome);
        if failed {
            break;
        }
    }
    outcomes
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

fn append_sequence_outcomes(
    results: &mut Vec<Value>,
    outcomes: Vec<SequenceOutcome>,
) -> Option<Value> {
    for outcome in outcomes {
        match outcome.result {
            Ok(Json(value)) => results.push(json!({
                "index": outcome.index,
                "tool": outcome.tool,
                "op": outcome.op,
                "ok": true,
                "result": value,
            })),
            Err(error) => {
                let mut error = error_json(error);
                if let Some(action) = error.pointer_mut("/data/next_action") {
                    prefix_sequence_action_fields(action, outcome.index);
                }
                let next_action = error.pointer("/data/next_action").cloned();
                return Some(json!({
                    "ok": false,
                    "phase": "runtime",
                    "executed": true,
                    "completed": results.len(),
                    "failed_index": outcome.index,
                    "results": results,
                    "error": error,
                    "next_action": next_action,
                }));
            }
        }
    }
    None
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
        _ => Err(invalid_input_error(
            "invalid tool for run_sequence; allowed values are: read, search, write, manage",
        )),
    }
}

#[cfg(test)]
mod tests;
