use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

/// One exact replacement.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
pub struct PatchEdit {
    /// The exact text to find (must match exactly once).
    pub old_text: String,
    /// The replacement text (must differ from `old_text`).
    pub new_text: String,
    /// Replacement mode: `unique` (default), `first`, or `all`.
    #[serde(default)]
    pub mode: Option<String>,
    /// Optional guard for the number of matches in the current text.
    #[serde(default)]
    pub expected_count: Option<usize>,
}

/// One line-based edit.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
pub struct LineEditInput {
    /// `insert_before_line`, `insert_after_line`, `replace_lines`, or `delete_lines`.
    pub op: String,
    /// 1-based line for insert operations.
    #[serde(default)]
    pub line: Option<i64>,
    /// 1-based first line for replace/delete operations.
    #[serde(default)]
    pub start_line: Option<i64>,
    /// 1-based last line for replace/delete operations.
    #[serde(default)]
    pub end_line: Option<i64>,
    /// Content to insert or replace with.
    #[serde(default)]
    pub content: Option<String>,
}

/// Public schema for `write.edits`; runtime parsing remains selected by the
/// top-level write operation.
#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
#[schemars(untagged, inline)]
pub enum WriteEditEntrySchema {
    Patch(PatchEdit),
    Line(LineEditInput),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
pub enum ReadOperationSchema {
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
pub enum SearchOperationSchema {
    Find,
    Grep,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
pub enum WriteOperationSchema {
    Write,
    Append,
    Patch,
    Edit,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
pub enum ManageOperationSchema {
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
    #[schemars(with = "ReadOperationSchema")]
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
    pub purpose: String,
    /// Operation: find/grep.
    #[schemars(with = "SearchOperationSchema")]
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
    pub purpose: String,
    /// Operation: write/append/patch/edit.
    #[schemars(with = "WriteOperationSchema")]
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
    pub purpose: String,
    /// Operation: mkdir/mv/cp/rm.
    #[schemars(with = "ManageOperationSchema")]
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

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
enum FileUploadOperationSchema {
    BeginUpload,
    PrepareParts,
    CompleteUpload,
    AbortUpload,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileUploadInput {
    /// Reason for this MCP invocation. Required once at the top level; maximum 200 characters.
    pub purpose: String,
    /// Operation: begin_upload/prepare_parts/complete_upload/abort_upload.
    #[schemars(with = "FileUploadOperationSchema")]
    pub op: String,
    /// Path-first target for begin_upload.
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
pub struct FileDownloadInput {
    /// Reason for this MCP invocation. Required once at the top level; maximum 200 characters.
    pub purpose: String,
    /// Path-first File target in `<space>:/absolute/path` form.
    pub target: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompletedPartInput {
    pub part_number: i32,
    pub etag: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn command_inputs_reject_unknown_fields() {
        let result = serde_json::from_value::<ReadInput>(json!({
            "purpose": "inspect",
            "op": "spaces",
            "unexpected": true,
        }));

        assert!(matches!(
            result,
            Err(error) if error.to_string().contains("unknown field `unexpected`")
        ));
    }

    #[test]
    fn search_match_field_keeps_its_public_name() -> Result<(), serde_json::Error> {
        let input = serde_json::from_value::<SearchInput>(json!({
            "purpose": "search",
            "op": "grep",
            "target": "daily:/",
            "q": "needle",
            "match": "literal",
        }))?;

        assert_eq!(input.match_mode.as_deref(), Some("literal"));
        Ok(())
    }
}
