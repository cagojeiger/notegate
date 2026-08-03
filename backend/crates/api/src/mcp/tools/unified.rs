//! Unified MCP tools: read/search/write/manage/run_sequence.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::resolve::{actionable_input_error, invalid_input_error, required_input};
use super::{events, files, search, spaces};
use crate::mcp::contract::{McpAction, error_json};
use crate::state::AppState;

const RUN_SEQUENCE_MAX_COMMANDS: usize = 20;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    /// Short human-readable reason for this MCP call. Maximum 200 characters.
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
    /// Short human-readable reason for this MCP call. Maximum 200 characters.
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
    /// Short human-readable reason for this MCP call. Maximum 200 characters.
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
    /// Short human-readable reason for this MCP call. Maximum 200 characters.
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
    /// Short human-readable reason for this MCP call. Maximum 200 characters.
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
    /// One short human-readable reason shared by the whole sequence. Maximum 200 characters.
    pub purpose: String,
    /// Ordered NoteGate commands to execute. Maximum 20.
    pub commands: Vec<SequenceCommand>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SequenceCommand {
    /// Tool category for this command: read/search/write/manage.
    pub tool: String,

    /// Operation to perform within the selected tool.
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

pub async fn read(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<ReadInput>,
) -> Result<Json<Value>, ErrorData> {
    validate_read_change_fields(&input)?;
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

pub async fn search(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<SearchInput>,
) -> Result<Json<Value>, ErrorData> {
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

    let mut results = Vec::with_capacity(input.commands.len());
    for (index, command) in input.commands.into_iter().enumerate() {
        let tool = command.tool.clone();
        let op = command.op.clone();
        let result = dispatch_command(state, parts, command, &input.purpose).await;
        match result {
            Ok(Json(value)) => results.push(json!({
                "index": index,
                "tool": tool,
                "op": op,
                "ok": true,
                "result": value,
            })),
            Err(error) => {
                return Ok(Json(json!({
                    "ok": false,
                    "completed": results.len(),
                    "failed_index": index,
                    "results": results,
                    "error": error_json(error),
                })));
            }
        }
    }

    Ok(Json(json!({
        "ok": true,
        "completed": results.len(),
        "failed_index": null,
        "results": results,
    })))
}

async fn dispatch_command(
    state: &AppState,
    parts: &Parts,
    command: SequenceCommand,
    purpose: &str,
) -> Result<Json<Value>, ErrorData> {
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
    invalid_input_error(format!(
        "invalid op for {tool}; allowed values are: {}",
        allowed.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

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
        let error = serde_json::from_value::<RunSequenceInput>(json!({
            "purpose": "test unknown sequence fields",
            "commands": [{
                "tool": "read",
                "op": "spaces",
                "unexpected": true
            }]
        }))
        .expect_err("unknown command field should be rejected");

        assert!(error.to_string().contains("unknown field"));
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

        assert_eq!(input.commands.len(), 1);
        let command = input.commands.first().expect("one command");
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

        let command = input.commands.first().expect("one command");
        assert_eq!(command.direction.as_deref(), Some("newer"));
        assert_eq!(command.cursor.as_deref(), Some("opaque-change-cursor"));
    }
}
