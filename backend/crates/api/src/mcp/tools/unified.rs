//! Unified MCP tools: read/search/write/manage/run_sequence.

use axum::http::request::Parts;
use notegate_core::validation::validate_space_name;
use notegate_service::ServiceError;
use notegate_service::files::{
    Target, content, parse_target, validate_structured_text,
    validation::{validate_basename, validate_text_content},
};
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
use crate::mcp::contract::McpAction;
use crate::state::AppState;

mod sequence;

pub use sequence::{RunSequenceInput, run_sequence};

/// Public schema for `write.edits`; runtime parsing remains selected by the top-level write op.
#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
#[schemars(untagged, inline)]
enum WriteEditEntrySchema {
    Patch(files::PatchEdit),
    Line(files::LineEditInput),
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
    /// Single target in `<space>:/absolute/path` form for mkdir/rm. The Space root is allowed only for mkdir with parents=true.
    #[serde(default)]
    pub target: Option<String>,
    /// Non-root source target for mv/cp.
    #[serde(default)]
    pub source: Option<String>,
    /// Non-root destination target for mv/cp.
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

fn validate_static_write_content(input: &WriteInput) -> Result<(), ErrorData> {
    let content = match input.op.as_str() {
        "write" | "append" => required_ref(input.content.as_ref(), "content", &input.op)?,
        _ => return Ok(()),
    };
    let metrics = content::compute(content);
    validate_text_content(metrics.byte_len, metrics.line_count)
        .map_err(ServiceError::from)
        .map_err(service_error)?;

    if input.op == "write" {
        let target = parse_input_target(&input.target)?;
        let (_, name) = split_parent_name(&target.path)?;
        validate_structured_text(&name, content).map_err(service_error)?;
    }
    Ok(())
}

fn validate_manage_operation(input: &ManageInput) -> Result<(), ErrorData> {
    match input.op.as_str() {
        "mkdir" => {
            let target =
                parse_input_target(required_ref(input.target.as_ref(), "target", "mkdir")?)?;
            if !input.parents {
                validate_non_root_target(&target)?;
            }
            Ok(())
        }
        "rm" => {
            let target = parse_input_target(required_ref(input.target.as_ref(), "target", "rm")?)?;
            validate_non_root_target(&target)?;
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
            validate_non_root_target(&source)?;
            validate_non_root_target(&destination)?;
            Ok(())
        }
        _ => Err(invalid_op("manage", &["mkdir", "mv", "cp", "rm"])),
    }
}

fn validate_non_root_target(target: &Target) -> Result<(), ErrorData> {
    split_parent_name(&target.path).map(|_| ())
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

fn required<T>(value: Option<T>, field: &'static str, op: &'static str) -> Result<T, ErrorData> {
    required_input(value, field, &format!("op={op}"))
}

fn required_ref<'a, T>(
    value: Option<&'a T>,
    field: &'static str,
    op: &str,
) -> Result<&'a T, ErrorData> {
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
    use serde_json::json;

    use super::{FileTransferInput, ManageInput, WriteInput};

    #[test]
    fn mutation_tools_reject_node_metadata_fields() {
        assert!(
            serde_json::from_value::<WriteInput>(json!({
                "purpose": "verify metadata boundary",
                "op": "write",
                "target": "daily:/note.md",
                "metadata": {}
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ManageInput>(json!({
                "purpose": "verify metadata boundary",
                "op": "mkdir",
                "target": "daily:/folder",
                "metadata": {}
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<FileTransferInput>(json!({
                "purpose": "verify metadata boundary",
                "op": "complete_upload",
                "upload_id": "upload-id",
                "node_metadata": {}
            }))
            .is_err()
        );
    }
}
