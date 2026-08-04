//! Unified MCP tools: read/search/write/manage/run_sequence.

use axum::http::request::Parts;
use futures_util::{StreamExt, stream};
use notegate_core::validation::validate_space_name;
use notegate_service::ServiceError;
use notegate_service::files::{Target, parse_target, validation::validate_basename};
use notegate_service::search::{validate_find_input, validate_grep_input};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::resolve::{
    actionable_input_error, invalid_input_error, required_input, service_error, split_parent_name,
};
use super::{events, files, search, spaces};
use crate::mcp::contract::{McpAction, McpErrorData, error_json};
use crate::state::AppState;

const RUN_SEQUENCE_MAX_COMMANDS: usize = 20;
const RUN_SEQUENCE_READ_CONCURRENCY: usize = 4;

/// Public schema for `write.edits`; runtime parsing remains selected by the top-level write op.
#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
#[schemars(untagged, inline)]
enum WriteEditEntrySchema {
    Patch(files::PatchEdit),
    Line(files::LineEditInput),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
enum SequenceToolSchema {
    Read,
    Search,
    Write,
    Manage,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
enum SequenceOperationSchema {
    Spaces,
    Ls,
    Tree,
    Stat,
    Read,
    Changes,
    Find,
    Grep,
    Write,
    Append,
    Patch,
    Edit,
    Mkdir,
    Mv,
    Cp,
    Rm,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    /// Reason for this MCP invocation. Required once at the top level; maximum 200 characters.
    pub purpose: String,
    /// Operation: spaces/ls/tree/stat/read/changes.
    pub op: String,
    /// Single target in `<space>:/absolute/path` form. `op=changes` requires the Space root `<space>:/`.
    #[serde(default)]
    pub target: Option<String>,
    /// Optional exact, case-sensitive space name filter for `op=spaces`.
    #[serde(default)]
    pub name: Option<String>,
    /// Tree depth for `op=tree`.
    #[serde(default)]
    pub depth: Option<i64>,
    /// Page size.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor.
    #[serde(default)]
    pub cursor: Option<String>,
    /// For `op=changes`, read older events (default) or newer events.
    #[serde(default)]
    pub direction: Option<String>,
    /// 1-based first line for `op=read`.
    #[serde(default)]
    pub start_line: Option<i64>,
    /// Maximum lines for `op=read`.
    #[serde(default)]
    pub max_lines: Option<i64>,
    /// Maximum bytes for `op=read`.
    #[serde(default)]
    pub max_bytes: Option<usize>,
    /// Conditional read guard.
    #[serde(default)]
    pub if_none_match_sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchInput {
    /// Reason for this MCP invocation. Required once at the top level; maximum 200 characters.
    #[allow(
        dead_code,
        reason = "validated and recorded at the shared tools/call boundary"
    )]
    pub purpose: String,
    /// Operation: find/grep.
    pub op: String,
    /// Scope target in `<space>:/absolute/path` form. The space name segment is exact and case-sensitive.
    pub target: String,
    /// Search query. `find` and `grep` matching inside the resolved space is case-insensitive.
    pub q: String,
    /// Node kind filter for `op=find`: folder/text/file.
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
    /// Page size.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor.
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteInput {
    /// Reason for this MCP invocation. Required once at the top level; maximum 200 characters.
    #[allow(
        dead_code,
        reason = "validated and recorded at the shared tools/call boundary"
    )]
    pub purpose: String,
    /// Operation: write/append/patch/edit.
    pub op: String,
    /// Text target in `<space>:/absolute/path` form.
    pub target: String,
    /// Text content for write/append.
    #[serde(default)]
    pub content: Option<String>,
    /// Patch or line-edit entries for patch/edit.
    #[serde(default)]
    #[schemars(with = "Option<Vec<WriteEditEntrySchema>>")]
    pub edits: Option<Vec<Value>>,
    /// Create missing text for write/append.
    #[serde(default)]
    pub create: bool,
    /// Insert a newline before appended content when needed.
    #[serde(default)]
    pub ensure_newline: bool,
    /// Optimistic write guard.
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManageInput {
    /// Reason for this MCP invocation. Required once at the top level; maximum 200 characters.
    #[allow(
        dead_code,
        reason = "validated and recorded at the shared tools/call boundary"
    )]
    pub purpose: String,
    /// Operation: mkdir/mv/cp/rm.
    pub op: String,
    /// Single target in `<space>:/absolute/path` form for mkdir/rm.
    #[serde(default)]
    pub target: Option<String>,
    /// Source target for mv/cp.
    #[serde(default)]
    pub source: Option<String>,
    /// Destination target for mv/cp.
    #[serde(default)]
    pub destination: Option<String>,
    /// Create missing parent folders for mkdir.
    #[serde(default)]
    pub parents: bool,
    /// Required for folder cp/rm.
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileTransferInput {
    /// Reason for this MCP invocation. Required once at the top level; maximum 200 characters.
    pub purpose: String,
    /// Operation: begin_upload/prepare_parts/complete_upload/abort_upload/prepare_download.
    pub op: String,
    /// Path-first target for begin_upload and prepare_download.
    #[serde(default)]
    pub target: Option<String>,
    /// Local file byte length for begin_upload.
    #[serde(default)]
    pub byte_len: Option<i64>,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub original_filename: Option<String>,
    #[serde(default)]
    pub encryption_mode: Option<String>,
    #[serde(default)]
    pub encryption_metadata: Option<Value>,
    /// Upload handle returned by begin_upload.
    #[serde(default)]
    pub upload_id: Option<String>,
    /// Multipart numbers to presign, at most 16 per call.
    #[serde(default)]
    pub part_numbers: Option<Vec<i32>>,
    /// Multipart ETags captured from successful PUT responses.
    #[serde(default)]
    pub completed_parts: Option<Vec<CompletedPartInput>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompletedPartInput {
    pub part_number: i32,
    pub etag: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunSequenceInput {
    /// Reason for this MCP invocation. Commands inherit it; maximum 200 characters.
    pub purpose: String,
    /// Ordered NoteGate commands to execute. Maximum 20.
    #[schemars(with = "Vec<SequenceCommand>")]
    pub commands: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
pub struct SequenceCommand {
    /// Tool category for this command: read/search/write/manage.
    #[schemars(with = "SequenceToolSchema")]
    pub tool: String,

    /// Operation for the selected tool: read=spaces/ls/tree/stat/read/changes, search=find/grep, write=write/append/patch/edit, manage=mkdir/mv/cp/rm.
    #[schemars(with = "SequenceOperationSchema")]
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
    #[schemars(with = "Option<Vec<WriteEditEntrySchema>>")]
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

    /// Optimistic write guard.
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

const SEQUENCE_COMMAND_FIELDS: &[&str] = &[
    "tool",
    "op",
    "target",
    "source",
    "destination",
    "name",
    "q",
    "kind",
    "match",
    "lines",
    "include",
    "exclude",
    "content",
    "edits",
    "create",
    "parents",
    "recursive",
    "ensure_newline",
    "depth",
    "limit",
    "cursor",
    "direction",
    "start_line",
    "max_lines",
    "max_bytes",
    "expected_sha256",
    "if_none_match_sha256",
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

pub async fn read(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<ReadInput>,
) -> Result<Json<Value>, ErrorData> {
    validate_read_operation(&input)?;
    match input.op.as_str() {
        "spaces" => spaces::list(state, parts, input.name, input.limit, input.cursor).await,
        "ls" => {
            files::list(
                state,
                parts,
                required(input.target, "target", "ls")?,
                Some(1),
                input.limit,
                input.cursor,
            )
            .await
        }
        "tree" => {
            files::list(
                state,
                parts,
                required(input.target, "target", "tree")?,
                Some(input.depth.unwrap_or(5)),
                input.limit,
                input.cursor,
            )
            .await
        }
        "stat" => files::stat(state, parts, required(input.target, "target", "stat")?).await,
        "read" => {
            files::read(
                state,
                parts,
                required(input.target, "target", "read")?,
                input.start_line,
                input.max_lines,
                input.max_bytes,
                input.if_none_match_sha256,
            )
            .await
        }
        "changes" => {
            events::call(
                state,
                parts,
                &input.purpose,
                required(input.target, "target", "changes")?,
                input.limit,
                input.direction,
                input.cursor,
            )
            .await
        }
        _ => Err(invalid_op(
            "read",
            &["spaces", "ls", "tree", "stat", "read", "changes"],
        )),
    }
}

fn validate_read_change_fields(input: &ReadInput) -> Result<(), ErrorData> {
    if input.op == "changes" {
        return Ok(());
    }
    if input.direction.is_some() {
        return Err(actionable_input_error(
            "changes_fields_not_allowed",
            "direction is only valid for read op=changes",
            "Remove direction or change op to changes.",
            McpAction::RemoveFields {
                fields: vec!["direction".to_owned()],
            },
        ));
    }
    Ok(())
}

fn validate_read_operation(input: &ReadInput) -> Result<(), ErrorData> {
    validate_read_change_fields(input)?;
    match input.op.as_str() {
        "spaces" => {
            if let Some(name) = input.name.as_deref() {
                validate_space_name(name)
                    .map_err(|error| invalid_input_error(error.to_string()))?;
            }
            Ok(())
        }
        "ls" | "stat" => {
            parse_input_target(required_ref(
                input.target.as_ref(),
                "target",
                input.op.as_str(),
            )?)?;
            Ok(())
        }
        "tree" => {
            parse_input_target(required_ref(input.target.as_ref(), "target", "tree")?)?;
            if input.depth.is_some_and(|depth| depth < 1) {
                return Err(invalid_input_error("depth must be at least 1"));
            }
            Ok(())
        }
        "read" => {
            parse_input_target(required_ref(input.target.as_ref(), "target", "read")?)?;
            if input.max_bytes == Some(0) {
                return Err(invalid_input_error("max_bytes must be at least 1"));
            }
            Ok(())
        }
        "changes" => {
            events::validate_input(
                required_ref(input.target.as_ref(), "target", "changes")?,
                input.direction.as_deref(),
                input.cursor.as_deref(),
                &input.purpose,
            )?;
            Ok(())
        }
        _ => Err(invalid_op(
            "read",
            &["spaces", "ls", "tree", "stat", "read", "changes"],
        )),
    }
}

fn validate_search_operation(input: &SearchInput) -> Result<(), ErrorData> {
    match input.op.as_str() {
        "find" => {
            parse_input_target(&input.target)?;
            if let Some(kind) = input.kind.as_deref() {
                search::parse_kind(kind)?;
            }
            let match_mode = search::parse_find_match_mode(input.match_mode.as_deref())?;
            validate_find_input(
                &input.q,
                match_mode,
                input.include.as_deref().unwrap_or_default(),
                input.exclude.as_deref().unwrap_or_default(),
            )
            .map_err(service_error)?;
            Ok(())
        }
        "grep" => {
            parse_input_target(&input.target)?;
            let match_mode = search::parse_grep_match_mode(input.match_mode.as_deref())?;
            search::parse_grep_line_mode(input.lines.as_deref())?;
            validate_grep_input(
                &input.q,
                match_mode,
                input.include.as_deref().unwrap_or_default(),
                input.exclude.as_deref().unwrap_or_default(),
            )
            .map_err(service_error)?;
            Ok(())
        }
        _ => Err(invalid_op("search", &["find", "grep"])),
    }
}

fn validate_write_operation(input: &WriteInput) -> Result<(), ErrorData> {
    match input.op.as_str() {
        "write" | "append" => {
            required_ref(input.content.as_ref(), "content", input.op.as_str())?;
            validate_text_target(&input.target)?;
            Ok(())
        }
        "patch" => {
            let edits = parse_edits::<files::PatchEdit>(input.edits.clone(), "patch")?;
            files::prepare_patch_edits(&edits)?;
            validate_text_target(&input.target)?;
            Ok(())
        }
        "edit" => {
            let edits = parse_edits::<files::LineEditInput>(input.edits.clone(), "edit")?;
            files::prepare_line_edits(&edits)?;
            validate_text_target(&input.target)?;
            Ok(())
        }
        _ => Err(invalid_op("write", &["write", "append", "patch", "edit"])),
    }
}

fn validate_manage_operation(input: &ManageInput) -> Result<(), ErrorData> {
    match input.op.as_str() {
        "mkdir" | "rm" => {
            parse_input_target(required_ref(
                input.target.as_ref(),
                "target",
                input.op.as_str(),
            )?)?;
            Ok(())
        }
        "mv" | "cp" => {
            let source = parse_input_target(required_ref(
                input.source.as_ref(),
                "source",
                input.op.as_str(),
            )?)?;
            let destination = parse_input_target(required_ref(
                input.destination.as_ref(),
                "destination",
                input.op.as_str(),
            )?)?;
            if source.space != destination.space {
                return Err(invalid_input_error(
                    "source and destination must be in the same space",
                ));
            }
            Ok(())
        }
        _ => Err(invalid_op("manage", &["mkdir", "mv", "cp", "rm"])),
    }
}

fn parse_input_target(target: &str) -> Result<Target, ErrorData> {
    let target = parse_target(target).map_err(|error| invalid_input_error(error.to_string()))?;
    for segment in target.path.split('/').filter(|segment| !segment.is_empty()) {
        validate_basename(segment)
            .map_err(ServiceError::from)
            .map_err(service_error)?;
    }
    Ok(target)
}

fn validate_text_target(target: &str) -> Result<(), ErrorData> {
    let target = parse_input_target(target)?;
    split_parent_name(&target.path)?;
    Ok(())
}

pub async fn search(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<SearchInput>,
) -> Result<Json<Value>, ErrorData> {
    validate_search_operation(&input)?;
    match input.op.as_str() {
        "find" => {
            search::find(
                state,
                parts,
                input.target,
                input.q,
                input.kind,
                input.match_mode,
                input.include,
                input.exclude,
                input.limit,
                input.cursor,
            )
            .await
        }
        "grep" => {
            search::grep(
                state,
                parts,
                input.target,
                input.q,
                input.match_mode,
                input.lines,
                input.include,
                input.exclude,
                input.limit,
                input.cursor,
            )
            .await
        }
        _ => Err(invalid_op("search", &["find", "grep"])),
    }
}

pub async fn write(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<WriteInput>,
) -> Result<Json<Value>, ErrorData> {
    validate_write_operation(&input)?;
    match input.op.as_str() {
        "write" => {
            files::write(
                state,
                parts,
                input.target,
                required(input.content, "content", "write")?,
                input.create,
                input.expected_sha256,
            )
            .await
        }
        "append" => {
            files::append(
                state,
                parts,
                input.target,
                required(input.content, "content", "append")?,
                input.create,
                input.ensure_newline,
                input.expected_sha256,
            )
            .await
        }
        "patch" => {
            files::patch(
                state,
                parts,
                input.target,
                parse_edits(input.edits, "patch")?,
                input.expected_sha256,
            )
            .await
        }
        "edit" => {
            files::edit(
                state,
                parts,
                input.target,
                parse_edits(input.edits, "edit")?,
                input.expected_sha256,
            )
            .await
        }
        _ => Err(invalid_op("write", &["write", "append", "patch", "edit"])),
    }
}

pub async fn manage(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<ManageInput>,
) -> Result<Json<Value>, ErrorData> {
    validate_manage_operation(&input)?;
    match input.op.as_str() {
        "mkdir" => {
            files::mkdir(
                state,
                parts,
                required(input.target, "target", "mkdir")?,
                input.parents,
            )
            .await
        }
        "mv" => {
            files::mv(
                state,
                parts,
                required(input.source, "source", "mv")?,
                required(input.destination, "destination", "mv")?,
            )
            .await
        }
        "cp" => {
            files::copy(
                state,
                parts,
                required(input.source, "source", "cp")?,
                required(input.destination, "destination", "cp")?,
                input.recursive,
            )
            .await
        }
        "rm" => {
            files::rm(
                state,
                parts,
                required(input.target, "target", "rm")?,
                input.recursive,
            )
            .await
        }
        _ => Err(invalid_op("manage", &["mkdir", "mv", "cp", "rm"])),
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
            .filter(|field| !SEQUENCE_COMMAND_FIELDS.contains(&field.as_str()))
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

        let missing_fields = ["tool", "op"]
            .into_iter()
            .filter(|field| !candidate.contains_key(*field))
            .map(|field| crate::mcp::contract::RequiredField {
                field: format!("commands[{index}].{field}"),
                description: None,
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
    data.details.insert("executed".to_owned(), json!(false));
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

fn required_ref<'a, T>(
    value: Option<&'a T>,
    field: &'static str,
    op: &str,
) -> Result<&'a T, ErrorData> {
    required_input(value, field, &format!("op={op}"))
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
    if input.commands.is_empty() {
        return Err(invalid_input_error(
            "run_sequence requires at least one command",
        ));
    }
    if input.commands.len() > RUN_SEQUENCE_MAX_COMMANDS {
        return Err(invalid_input_error(format!(
            "run_sequence accepts at most {RUN_SEQUENCE_MAX_COMMANDS} commands"
        )));
    }

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
                return Some(json!({
                    "ok": false,
                    "completed": results.len(),
                    "failed_index": outcome.index,
                    "results": results,
                    "error": error_json(error),
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

fn required<T>(value: Option<T>, field: &'static str, op: &'static str) -> Result<T, ErrorData> {
    required_input(value, field, &format!("op={op}"))
}

fn parse_edits<T>(value: Option<Vec<Value>>, op: &'static str) -> Result<Vec<T>, ErrorData>
where
    T: serde::de::DeserializeOwned,
{
    let edits = value.ok_or_else(|| {
        invalid_input_error(format!(
            "op={op} requires edits; retry with a non-empty `edits` array"
        ))
    })?;
    edits
        .into_iter()
        .map(|edit| {
            serde_json::from_value(edit).map_err(|error| {
                invalid_input_error(format!("invalid edit entry for op={op}: {error}"))
            })
        })
        .collect()
}

fn invalid_op(tool: &'static str, allowed: &[&str]) -> ErrorData {
    actionable_input_error(
        "invalid_op",
        format!(
            "invalid op for {tool}; allowed values are: {}",
            allowed.join(", ")
        ),
        "Choose one of the operation values listed by next_action.choices.",
        McpAction::ChooseValue {
            field: "op".to_owned(),
            choices: allowed.iter().map(|value| json!(value)).collect(),
        },
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use std::collections::BTreeSet;

    use super::*;
    use serde_json::json;

    #[test]
    fn operation_specific_required_fields_use_the_common_recovery_action() {
        let error = required::<String>(None, "target", "read").expect_err("target is required");
        let data = error.data.expect("missing field carries recovery data");

        assert_eq!(data["code"], "required_field_missing");
        assert_eq!(data["next_action"]["kind"], "add_fields");
        assert_eq!(data["next_action"]["fields"][0]["field"], "target");
    }

    #[test]
    fn purpose_is_required_for_direct_and_sequence_calls() {
        let direct = serde_json::from_value::<SearchInput>(json!({
            "op": "find",
            "target": "daily:/",
            "q": "cache"
        }));
        assert!(direct.is_err());

        let sequence = serde_json::from_value::<RunSequenceInput>(json!({
            "commands": [{"tool": "read", "op": "spaces"}]
        }));
        assert!(sequence.is_err());
    }

    #[test]
    fn sequence_command_rejects_unknown_fields() {
        let input = serde_json::from_value::<RunSequenceInput>(json!({
            "purpose": "test unknown sequence fields",
            "commands": [{
                "tool": "read",
                "op": "spaces",
                "unexpected": true
            }]
        }))
        .expect("raw sequence commands parse before preflight");
        let error = prepare_sequence_commands(input.commands, &input.purpose)
            .expect_err("unknown command field should be rejected by preflight");
        let data = error.data.expect("structured preflight data");

        assert_eq!(data["code"], "sequence_preflight_failed");
        assert_eq!(data["executed"], false);
        assert_eq!(data["next_action"]["kind"], "apply_error_actions");
        assert_eq!(data["next_action"]["errors_field"], "errors");
        assert_eq!(data["errors"][0]["code"], "sequence_command_unknown_fields");
        assert_eq!(
            data["errors"][0]["next_action"]["fields"][0],
            "commands[0].unexpected"
        );
    }

    #[test]
    fn sequence_preflight_allowlist_matches_the_public_command_schema() {
        let schema = json!(schemars::schema_for!(SequenceCommand));
        let properties = schema["properties"]
            .as_object()
            .expect("sequence properties");
        let schema_fields = properties
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let preflight_fields = SEQUENCE_COMMAND_FIELDS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();

        assert_eq!(preflight_fields, schema_fields);
    }

    #[test]
    fn sequence_preflight_explains_top_level_purpose_and_flat_commands() {
        let input = serde_json::from_value::<RunSequenceInput>(json!({
            "purpose": "read two notes",
            "commands": [
                {
                    "tool": "read",
                    "op": "read",
                    "target": "daily:/one.md",
                    "purpose": "incorrect nested purpose"
                },
                {
                    "tool": "read",
                    "args": {
                        "purpose": "incorrect direct-tool purpose",
                        "op": "read",
                        "target": "daily:/two.md"
                    }
                }
            ]
        }))
        .expect("raw commands parse before preflight");
        let error = prepare_sequence_commands(input.commands, &input.purpose)
            .expect_err("both command shapes should fail preflight");
        let data = error.data.expect("structured preflight data");

        assert_eq!(data["executed"], false);
        assert_eq!(data["errors"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            data["errors"][0]["code"],
            "sequence_command_purpose_not_allowed"
        );
        assert_eq!(
            data["errors"][0]["next_action"]["fields"][0],
            "commands[0].purpose"
        );
        assert_eq!(
            data["errors"][1]["code"],
            "sequence_command_args_not_allowed"
        );
        assert_eq!(data["errors"][1]["next_action"]["kind"], "replace_field");
        assert_eq!(
            data["errors"][1]["next_action"]["value"],
            json!({"tool": "read", "op": "read", "target": "daily:/two.md"})
        );
    }

    #[test]
    fn sequence_preflight_collects_later_static_errors_before_execution() {
        let input = serde_json::from_value::<RunSequenceInput>(json!({
            "purpose": "validate the entire sequence",
            "commands": [
                {
                    "tool": "write",
                    "op": "write",
                    "target": "daily:/created.md",
                    "content": "created"
                },
                {"tool": "search", "op": "find", "target": "daily:/"},
                {"tool": "manage", "op": "mv", "source": "daily:/from.md"}
            ]
        }))
        .expect("raw commands parse before preflight");
        let error = prepare_sequence_commands(input.commands, &input.purpose)
            .expect_err("missing q and destination should fail before execution");
        let data = error.data.expect("structured preflight data");

        assert_eq!(data["executed"], false);
        assert_eq!(data["errors"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            data["errors"][0]["next_action"]["fields"][0]["field"],
            "commands[1].q"
        );
        assert_eq!(
            data["errors"][1]["next_action"]["fields"][0]["field"],
            "commands[2].destination"
        );
    }

    #[test]
    fn sequence_preflight_rejects_invalid_changes_before_a_prior_write_executes() {
        let input = serde_json::from_value::<RunSequenceInput>(json!({
            "purpose": "validate changes before updating a note",
            "commands": [
                {
                    "tool": "write",
                    "op": "write",
                    "target": "daily:/created.md",
                    "content": "created"
                },
                {
                    "tool": "read",
                    "op": "changes",
                    "target": "daily:/",
                    "direction": "latest"
                },
                {
                    "tool": "read",
                    "op": "changes",
                    "target": "daily:/",
                    "direction": "newer"
                },
                {
                    "tool": "read",
                    "op": "changes",
                    "target": "daily:/folder"
                }
            ]
        }))
        .expect("raw commands parse before preflight");
        let error = prepare_sequence_commands(input.commands, &input.purpose)
            .expect_err("all invalid changes commands must fail before execution");
        let data = error.data.expect("structured preflight data");

        assert_eq!(data["executed"], false);
        assert_eq!(data["errors"].as_array().map(Vec::len), Some(3));
        assert_eq!(data["errors"][0]["index"], 1);
        assert_eq!(data["errors"][0]["code"], "changes_direction_invalid");
        assert_eq!(
            data["errors"][0]["next_action"]["field"],
            "commands[1].direction"
        );
        assert_eq!(data["errors"][1]["index"], 2);
        assert_eq!(data["errors"][1]["code"], "changes_cursor_required");
        assert_eq!(data["errors"][2]["index"], 3);
        assert_eq!(data["errors"][2]["code"], "changes_scope_invalid");
        assert_eq!(
            data["errors"][2]["next_action"]["field"],
            "commands[3].target"
        );
    }

    #[test]
    fn sequence_preflight_rejects_input_only_errors_before_a_prior_write_executes() {
        let cases = vec![
            (
                json!({"tool": "search", "op": "find", "target": "daily:/", "q": ""}),
                "search query cannot be empty",
            ),
            (
                json!({"tool": "search", "op": "grep", "target": "daily:/", "q": "(", "match": "regex"}),
                "invalid regex pattern",
            ),
            (
                json!({"tool": "write", "op": "patch", "target": "daily:/note.md", "edits": [{"old_text": "before", "new_text": "after", "mode": "latest"}]}),
                "mode must be 'unique', 'first', or 'all'",
            ),
            (
                json!({"tool": "write", "op": "patch", "target": "daily:/note.md", "edits": []}),
                "edits must not be empty",
            ),
            (
                json!({"tool": "write", "op": "edit", "target": "daily:/note.md", "edits": [{"op": "delete_lines"}]}),
                "start_line is required",
            ),
            (
                json!({"tool": "write", "op": "edit", "target": "daily:/note.md", "edits": [{"op": "delete_lines", "start_line": 3, "end_line": 2}]}),
                "start_line must be less than or equal to end_line",
            ),
            (
                json!({"tool": "manage", "op": "mv", "source": "daily:/from.md", "destination": "other:/to.md"}),
                "source and destination must be in the same space",
            ),
            (
                json!({"tool": "manage", "op": "cp", "source": "daily:/from.md", "destination": "other:/to.md"}),
                "source and destination must be in the same space",
            ),
            (
                json!({"tool": "read", "op": "tree", "target": "daily:/", "depth": 0}),
                "depth must be at least 1",
            ),
            (
                json!({"tool": "read", "op": "read", "target": "daily:/note.md", "max_bytes": 0}),
                "max_bytes must be at least 1",
            ),
        ];

        for (invalid_command, expected_message) in cases {
            let error = prepare_sequence_commands(
                vec![
                    json!({
                        "tool": "write",
                        "op": "write",
                        "target": "daily:/created.md",
                        "content": "created",
                        "create": true
                    }),
                    invalid_command,
                ],
                "reject request-local errors before writing",
            )
            .expect_err("request-local errors must fail sequence preflight");
            let data = error.data.expect("structured preflight data");

            assert_eq!(data["executed"], false);
            assert_eq!(data["errors"].as_array().map(Vec::len), Some(1));
            assert_eq!(data["errors"][0]["index"], 1);
            assert!(
                data["errors"][0]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(expected_message)),
                "expected error message containing {expected_message:?}, got {}",
                data["errors"][0]["message"]
            );
        }
    }

    #[test]
    fn sequence_preflight_collects_recoverable_shape_and_value_errors_in_one_command() {
        let error = prepare_sequence_commands(
            vec![json!({
                "tool": "search",
                "op": "grep",
                "target": "daily:/",
                "q": "cache",
                "match": "glob",
                "purpose": "incorrect nested purpose",
                "unexpected": true
            })],
            "validate every static input error",
        )
        .expect_err("all recoverable static errors should be reported together");
        let data = error.data.expect("structured preflight data");
        let codes = data["errors"]
            .as_array()
            .expect("errors array")
            .iter()
            .map(|error| error["code"].as_str().expect("error code"))
            .collect::<Vec<_>>();

        assert_eq!(
            codes,
            vec![
                "sequence_command_purpose_not_allowed",
                "sequence_command_unknown_fields",
                "invalid_field_value"
            ]
        );
        assert_eq!(data["executed"], false);
    }

    #[test]
    fn sequence_preflight_names_valid_tool_and_operation_choices() {
        let error = prepare_sequence_commands(
            vec![
                json!({"tool": "download", "op": "read"}),
                json!({"tool": "read", "op": "download", "target": "daily:/note.md"}),
                json!({"tool": "search", "op": "grep", "target": "daily:/", "q": "cache", "match": "glob"}),
            ],
            "validate command choices",
        )
        .expect_err("invalid tool and op should fail preflight");
        let data = error.data.expect("structured preflight data");

        assert_eq!(
            data["errors"][0]["next_action"]["field"],
            "commands[0].tool"
        );
        assert_eq!(data["errors"][1]["next_action"]["field"], "commands[1].op");
        assert_eq!(
            data["errors"][1]["next_action"]["choices"],
            json!(["spaces", "ls", "tree", "stat", "read", "changes"])
        );
        assert_eq!(
            data["errors"][2]["next_action"]["field"],
            "commands[2].match"
        );
        assert_eq!(
            data["errors"][2]["next_action"]["choices"],
            json!(["literal", "regex"])
        );
    }

    #[test]
    fn direct_and_sequence_commands_share_operation_validation() {
        let read = serde_json::from_value::<ReadInput>(json!({
            "purpose": "validate a direct read",
            "op": "read",
            "target": "daily:/note.md",
            "direction": "older"
        }))
        .expect("read input parses");
        let search = serde_json::from_value::<SearchInput>(json!({
            "purpose": "validate a direct search",
            "op": "grep",
            "target": "daily:/",
            "q": "cache",
            "match": "glob"
        }))
        .expect("search input parses");
        let write = serde_json::from_value::<WriteInput>(json!({
            "purpose": "validate a direct write",
            "op": "write",
            "target": "daily:/note.md"
        }))
        .expect("write input parses");
        let manage = serde_json::from_value::<ManageInput>(json!({
            "purpose": "validate a direct move",
            "op": "mv",
            "source": "daily:/from.md"
        }))
        .expect("manage input parses");

        let cases = vec![
            (
                validate_read_operation(&read).expect_err("direction is changes-only"),
                json!({"tool": "read", "op": "read", "target": "daily:/note.md", "direction": "older"}),
            ),
            (
                validate_search_operation(&search).expect_err("glob is invalid for grep"),
                json!({"tool": "search", "op": "grep", "target": "daily:/", "q": "cache", "match": "glob"}),
            ),
            (
                validate_write_operation(&write).expect_err("write content is required"),
                json!({"tool": "write", "op": "write", "target": "daily:/note.md"}),
            ),
            (
                validate_manage_operation(&manage).expect_err("move destination is required"),
                json!({"tool": "manage", "op": "mv", "source": "daily:/from.md"}),
            ),
        ];

        for (direct_error, command) in cases {
            let direct = error_json(direct_error);
            let sequence_error = prepare_sequence_commands(
                vec![command],
                "validate the same operation in a sequence",
            )
            .expect_err("sequence command uses the same validation");
            let sequence_data = sequence_error.data.expect("sequence error data");
            let sequence = &sequence_data["errors"][0];

            assert_eq!(sequence["code"], direct["data"]["code"]);
            let mut expected_action = direct["data"]["next_action"].clone();
            prefix_sequence_action_fields(&mut expected_action, 0);
            assert_eq!(sequence["next_action"], expected_action);
        }
    }

    #[test]
    fn edit_entries_keep_op_specific_runtime_parsing() {
        let patch = parse_edits::<files::PatchEdit>(
            Some(vec![json!({
                "old_text": "before",
                "new_text": "after",
                "mode": "unique",
                "expected_count": 1
            })]),
            "patch",
        )
        .expect("patch edit parses");
        assert_eq!(patch.len(), 1);
        assert_eq!(patch[0].old_text, "before");

        let line = parse_edits::<files::LineEditInput>(
            Some(vec![json!({
                "op": "replace_lines",
                "start_line": 2,
                "end_line": 3,
                "content": "replacement"
            })]),
            "edit",
        )
        .expect("line edit parses");
        assert_eq!(line.len(), 1);
        assert_eq!(line[0].op, "replace_lines");

        let error = parse_edits::<files::PatchEdit>(
            Some(vec![
                json!({"op": "delete_lines", "start_line": 2, "end_line": 3}),
            ]),
            "patch",
        )
        .expect_err("line edit must not parse as a patch edit");
        assert!(error.message.contains("invalid edit entry for op=patch"));
    }

    #[test]
    fn sequence_command_uses_direct_command_shape() {
        let input = serde_json::from_value::<RunSequenceInput>(json!({
            "purpose": "test direct sequence command shape",
            "commands": [{
                "tool": "manage",
                "op": "mkdir",
                "target": "main:/daily",
                "parents": true
            }]
        }))
        .expect("valid command sequence parses");

        let commands = prepare_sequence_commands(input.commands, &input.purpose)
            .expect("valid command sequence passes preflight");
        assert_eq!(commands.len(), 1);
        let command = &commands.first().expect("one command").command;
        assert_eq!(command.tool, "manage");
        assert_eq!(command.op, "mkdir");
        assert!(command.parents);
    }

    #[test]
    fn changes_uses_the_shared_opaque_cursor_with_a_direction() {
        let input = serde_json::from_value::<ReadInput>(json!({
            "purpose": "test changes pagination",
            "op": "changes",
            "target": "daily:/",
            "direction": "newer",
            "cursor": "opaque-change-cursor",
            "limit": 25
        }))
        .expect("valid changes input parses");

        assert_eq!(input.direction.as_deref(), Some("newer"));
        assert_eq!(input.cursor.as_deref(), Some("opaque-change-cursor"));
    }

    #[test]
    fn changes_fields_on_other_read_ops_name_the_fields_to_remove() {
        let input = serde_json::from_value::<ReadInput>(json!({
            "purpose": "test changes-only field validation",
            "op": "read",
            "target": "daily:/note.md",
            "direction": "older"
        }))
        .expect("known fields parse before operation validation");
        let error = validate_read_change_fields(&input)
            .expect_err("changes fields are rejected outside changes");

        let data = error.data.expect("structured recovery data");
        assert_eq!(data["code"], "changes_fields_not_allowed");
        assert_eq!(data["next_action"]["kind"], "remove_fields");
        assert_eq!(data["next_action"]["fields"], json!(["direction"]));
    }

    #[test]
    fn run_sequence_accepts_a_changes_cursor() {
        let input = serde_json::from_value::<RunSequenceInput>(json!({
            "purpose": "test changes in a sequence",
            "commands": [{
                "tool": "read",
                "op": "changes",
                "target": "daily:/",
                "direction": "newer",
                "cursor": "opaque-change-cursor"
            }]
        }))
        .expect("valid changes sequence parses");

        let commands = prepare_sequence_commands(input.commands, &input.purpose)
            .expect("valid changes sequence passes preflight");
        let command = &commands.first().expect("one command").command;
        assert_eq!(command.direction.as_deref(), Some("newer"));
        assert_eq!(command.cursor.as_deref(), Some("opaque-change-cursor"));
    }

    #[test]
    fn sequence_access_conflicts_are_path_and_scope_aware() {
        let purpose = "plan safe sequence concurrency";
        let commands = prepare_sequence_commands(
            vec![
                json!({"tool": "write", "op": "write", "target": "daily:/a/note.md", "content": "x"}),
                json!({"tool": "read", "op": "read", "target": "daily:/b/note.md"}),
                json!({"tool": "search", "op": "grep", "target": "daily:/a", "q": "x"}),
                json!({"tool": "read", "op": "changes", "target": "daily:/"}),
                json!({"tool": "read", "op": "read", "target": "other:/a/note.md"})
            ],
            purpose,
        )
        .expect("commands pass preflight");

        assert!(!sequence_commands_conflict(&commands[0], &commands[1]));
        assert!(sequence_commands_conflict(&commands[0], &commands[2]));
        assert!(sequence_commands_conflict(&commands[0], &commands[3]));
        assert!(!sequence_commands_conflict(&commands[0], &commands[4]));
    }

    #[test]
    fn parent_stat_depends_on_child_creation() {
        for mutation in [
            json!({"tool": "manage", "op": "mkdir", "target": "daily:/folder/child"}),
            json!({"tool": "write", "op": "write", "target": "daily:/folder/child.md", "content": "x", "create": true}),
        ] {
            let commands = prepare_sequence_commands(
                vec![
                    mutation,
                    json!({"tool": "read", "op": "stat", "target": "daily:/folder"}),
                    json!({"tool": "read", "op": "read", "target": "daily:/folder/sibling.md"}),
                ],
                "preserve parent metadata after child creation",
            )
            .expect("commands pass preflight");
            let graph = build_sequence_dependency_graph(&commands);

            assert!(graph.depends_on(1, 0));
            assert!(!graph.depends_on(2, 0));
        }
    }

    #[test]
    fn sequence_commands_are_classified_before_graph_construction() {
        let commands = prepare_sequence_commands(
            vec![
                json!({"tool": "read", "op": "read", "target": "daily:/note.md"}),
                json!({"tool": "search", "op": "grep", "target": "daily:/", "q": "cache"}),
                json!({"tool": "read", "op": "changes", "target": "daily:/"}),
                json!({"tool": "write", "op": "write", "target": "daily:/note.md", "content": "x"}),
                json!({"tool": "write", "op": "write", "target": "daily:/new.md", "content": "x", "create": true}),
                json!({"tool": "manage", "op": "mkdir", "target": "daily:/folder"}),
                json!({"tool": "manage", "op": "mkdir", "target": "daily:/a/b", "parents": true}),
                json!({"tool": "manage", "op": "mv", "source": "daily:/a", "destination": "daily:/b"}),
                json!({"tool": "manage", "op": "cp", "source": "daily:/a", "destination": "daily:/b"}),
                json!({"tool": "manage", "op": "rm", "target": "daily:/a", "recursive": true})
            ],
            "classify sequence execution",
        )
        .expect("commands pass preflight");

        assert_eq!(
            commands[0].execution_class,
            SequenceExecutionClass::PureRead
        );
        assert_eq!(
            commands[1].execution_class,
            SequenceExecutionClass::WideRead
        );
        assert_eq!(
            commands[2].execution_class,
            SequenceExecutionClass::ConsistencyRead
        );
        assert_eq!(
            commands[3].execution_class,
            SequenceExecutionClass::PointMutation
        );
        assert_eq!(
            commands[4].execution_class,
            SequenceExecutionClass::NamespaceMutation
        );
        assert_eq!(
            commands[5].execution_class,
            SequenceExecutionClass::NamespaceMutation
        );
        for command in &commands[6..] {
            assert_eq!(
                command.execution_class,
                SequenceExecutionClass::StructuralBarrier
            );
        }
        assert_eq!(commands[7].accesses.len(), 2);
        assert!(
            commands[7]
                .accesses
                .iter()
                .all(|access| access.mode == SequenceAccessMode::Write)
        );
        assert_eq!(commands[8].accesses.len(), 2);
        assert_eq!(commands[8].accesses[0].mode, SequenceAccessMode::Read);
        assert_eq!(commands[8].accesses[1].mode, SequenceAccessMode::Write);
    }

    #[test]
    fn dependency_graph_is_explicit_and_structural_mutations_are_barriers() {
        let commands = prepare_sequence_commands(
            vec![
                json!({"tool": "read", "op": "read", "target": "daily:/before.md"}),
                json!({"tool": "write", "op": "write", "target": "daily:/point.md", "content": "x"}),
                json!({"tool": "read", "op": "read", "target": "daily:/other.md"}),
                json!({"tool": "manage", "op": "cp", "source": "daily:/source", "destination": "daily:/destination", "recursive": true}),
                json!({"tool": "read", "op": "read", "target": "other:/after.md"}),
                json!({"tool": "write", "op": "write", "target": "daily:/last.md", "content": "x"})
            ],
            "build a deterministic dependency graph",
        )
        .expect("commands pass preflight");
        let graph = build_sequence_dependency_graph(&commands);

        assert_eq!(
            graph.dependencies,
            vec![
                vec![],
                vec![0],
                vec![],
                vec![0, 1, 2],
                vec![3],
                vec![0, 1, 2, 3, 4],
            ]
        );
        for (index, dependencies) in graph.dependencies.iter().enumerate() {
            assert!(dependencies.iter().all(|dependency| *dependency < index));
        }
    }
}
