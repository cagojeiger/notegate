//! Unified MCP tools: read/search/write/manage.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::resolve::invalid_input_error;
use super::{files, search, spaces};
use crate::state::AppState;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnifiedInput {
    /// Operation to perform within this tool.
    pub op: String,

    /// Single target in `<space>:/absolute/path` form.
    #[serde(default)]
    pub target: Option<String>,
    /// Source target for `mv` and `cp`.
    #[serde(default)]
    pub source: Option<String>,
    /// Destination target for `mv` and `cp`.
    #[serde(default)]
    pub destination: Option<String>,

    /// Optional exact space name filter for `read op=spaces`.
    #[serde(default)]
    pub name: Option<String>,
    /// Search query for `find` and `grep`.
    #[serde(default)]
    pub q: Option<String>,
    /// Node kind filter: `folder`, `text`, or `file`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Match mode. `find`: contains/regex/glob. `grep`: literal/regex.
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
    /// Opaque pagination cursor.
    #[serde(default)]
    pub cursor: Option<String>,

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

pub async fn read(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<UnifiedInput>,
) -> Result<Json<Value>, ErrorData> {
    match input.op.as_str() {
        "spaces" => {
            spaces::list(
                state,
                parts,
                Parameters(spaces::ListInput {
                    name: input.name,
                    limit: input.limit,
                    cursor: input.cursor,
                }),
            )
            .await
        }
        "ls" => {
            files::list(
                state,
                parts,
                Parameters(files::ListInput {
                    target: required(input.target, "target", "ls")?,
                    depth: Some(1),
                    limit: input.limit,
                    cursor: input.cursor,
                }),
            )
            .await
        }
        "tree" => {
            files::list(
                state,
                parts,
                Parameters(files::ListInput {
                    target: required(input.target, "target", "tree")?,
                    depth: Some(input.depth.unwrap_or(5)),
                    limit: input.limit,
                    cursor: input.cursor,
                }),
            )
            .await
        }
        "stat" => {
            files::stat(
                state,
                parts,
                Parameters(files::StatInput {
                    target: required(input.target, "target", "stat")?,
                }),
            )
            .await
        }
        "read" => {
            files::read(
                state,
                parts,
                Parameters(files::ReadInput {
                    target: required(input.target, "target", "read")?,
                    start_line: input.start_line,
                    max_lines: input.max_lines,
                    max_bytes: input.max_bytes,
                    if_none_match_sha256: input.if_none_match_sha256,
                }),
            )
            .await
        }
        _ => Err(invalid_op(
            "read",
            &["spaces", "ls", "tree", "stat", "read"],
        )),
    }
}

pub async fn search(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<UnifiedInput>,
) -> Result<Json<Value>, ErrorData> {
    match input.op.as_str() {
        "find" => {
            search::find(
                state,
                parts,
                Parameters(search::FindInput {
                    target: required(input.target, "target", "find")?,
                    q: required(input.q, "q", "find")?,
                    kind: input.kind,
                    match_mode: input.match_mode,
                    include: input.include,
                    exclude: input.exclude,
                    limit: input.limit,
                    cursor: input.cursor,
                }),
            )
            .await
        }
        "grep" => {
            search::grep(
                state,
                parts,
                Parameters(search::GrepInput {
                    target: required(input.target, "target", "grep")?,
                    q: required(input.q, "q", "grep")?,
                    match_mode: input.match_mode,
                    lines: input.lines,
                    include: input.include,
                    exclude: input.exclude,
                    limit: input.limit,
                    cursor: input.cursor,
                }),
            )
            .await
        }
        _ => Err(invalid_op("search", &["find", "grep"])),
    }
}

pub async fn write(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<UnifiedInput>,
) -> Result<Json<Value>, ErrorData> {
    match input.op.as_str() {
        "write" => {
            files::write(
                state,
                parts,
                Parameters(files::WriteInput {
                    target: required(input.target, "target", "write")?,
                    content: required(input.content, "content", "write")?,
                    create: input.create,
                    expected_sha256: input.expected_sha256,
                }),
            )
            .await
        }
        "append" => {
            files::append(
                state,
                parts,
                Parameters(files::AppendInput {
                    target: required(input.target, "target", "append")?,
                    content: required(input.content, "content", "append")?,
                    create: input.create,
                    ensure_newline: input.ensure_newline,
                    expected_sha256: input.expected_sha256,
                }),
            )
            .await
        }
        "patch" => {
            files::patch(
                state,
                parts,
                Parameters(files::PatchInput {
                    target: required(input.target, "target", "patch")?,
                    edits: parse_edits(input.edits, "patch")?,
                    expected_sha256: input.expected_sha256,
                }),
            )
            .await
        }
        "edit" => {
            files::edit(
                state,
                parts,
                Parameters(files::EditInput {
                    target: required(input.target, "target", "edit")?,
                    edits: parse_edits(input.edits, "edit")?,
                    expected_sha256: input.expected_sha256,
                }),
            )
            .await
        }
        _ => Err(invalid_op("write", &["write", "append", "patch", "edit"])),
    }
}

pub async fn manage(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<UnifiedInput>,
) -> Result<Json<Value>, ErrorData> {
    match input.op.as_str() {
        "mkdir" => {
            files::mkdir(
                state,
                parts,
                Parameters(files::MkdirInput {
                    target: required(input.target, "target", "mkdir")?,
                    parents: input.parents,
                }),
            )
            .await
        }
        "mv" => {
            files::mv(
                state,
                parts,
                Parameters(files::MvInput {
                    source: required(input.source, "source", "mv")?,
                    destination: required(input.destination, "destination", "mv")?,
                }),
            )
            .await
        }
        "cp" => {
            files::copy(
                state,
                parts,
                Parameters(files::CopyInput {
                    source: required(input.source, "source", "cp")?,
                    destination: required(input.destination, "destination", "cp")?,
                    recursive: input.recursive,
                }),
            )
            .await
        }
        "rm" => {
            files::rm(
                state,
                parts,
                Parameters(files::RmInput {
                    target: required(input.target, "target", "rm")?,
                    recursive: input.recursive,
                }),
            )
            .await
        }
        _ => Err(invalid_op("manage", &["mkdir", "mv", "cp", "rm"])),
    }
}

fn required(
    value: Option<String>,
    field: &'static str,
    op: &'static str,
) -> Result<String, ErrorData> {
    value.ok_or_else(|| {
        invalid_input_error(format!(
            "op={op} requires {field}; retry with field `{field}` set"
        ))
    })
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
