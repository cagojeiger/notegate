use axum::extract::{Extension, Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_model::{Caller, TextStorageFormat};
use notegate_service::ServiceError;
use notegate_service::files::{
    AppendText, Edit as ServiceEdit, EditText, LineEdit, PatchMode, PatchResult, PatchText,
    ReadResult, ReadText, ReadTextBody, TextView, WriteTarget, WriteText, WriteTextBody,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::agent_text::guarded_plain_text_sha;
use crate::error::ApiError;
use crate::state::AppState;

use super::dto::NodeOut;

const ENCRYPTED_TEXT_MESSAGE: &str = "encrypted text is not available through the Agent API";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/spaces/{space_id}/text/{node_id}",
            get(read).put(replace).patch(patch),
        )
        .route("/spaces/{space_id}/text/{node_id}/append", post(append))
        .route("/spaces/{space_id}/text/{node_id}/edit", post(edit))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReadQuery {
    start_line: Option<i64>,
    max_lines: Option<i64>,
    max_bytes: Option<usize>,
    if_none_match_sha256: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
/// Plain-text read result. `text` contains either a content page or an unchanged marker.
pub(crate) struct ReadResponse {
    node: NodeOut,
    text: ReadTextOut,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(untagged)]
/// Conditional reads return `ReadUnchangedOut`; other reads return `ReadContentOut`.
pub(crate) enum ReadTextOut {
    Content(ReadContentOut),
    Unchanged(ReadUnchangedOut),
}

#[derive(Debug, Serialize, ToSchema)]
/// A bounded page of UTF-8 text and the cursor for the next line range.
pub(crate) struct ReadContentOut {
    /// Returned UTF-8 content.
    content: String,
    /// SHA-256 of the complete stored text, not only this page.
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
    start_line: i64,
    end_line: i64,
    returned_lines: i64,
    /// Whether more content remains after this response.
    truncated: bool,
    /// Pass this value as `start_line` to continue, or null when complete.
    next_start_line: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
/// Returned when `if_none_match_sha256` equals the current complete-text hash.
pub(crate) struct ReadUnchangedOut {
    unchanged: bool,
    content_returned: bool,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
}

#[utoipa::path(
    get,
    path = "/api/v2/spaces/{space_id}/text/{node_id}",
    tag = "text",
    params(
        ("space_id" = Uuid, Path),
        ("node_id" = Uuid, Path),
        ("start_line" = Option<i64>, Query, description = "1-based first line"),
        ("max_lines" = Option<i64>, Query, description = "Maximum lines; defaults to 200 and is capped at 5000"),
        ("max_bytes" = Option<usize>, Query, description = "Maximum UTF-8 bytes; defaults to 65536 and is capped at 1048576"),
        ("if_none_match_sha256" = Option<String>, Query, description = "Return an unchanged marker instead of content when this complete-text SHA-256 still matches"),
    ),
    responses((status = 200, description = "Read plain text", body = ReadResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn read(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ReadQuery>,
) -> Result<Json<ReadResponse>, ApiError> {
    let result = state
        .files
        .read_text(
            caller.account_id(),
            space_id,
            ReadText {
                node_id,
                start_line: query.start_line,
                max_lines: query.max_lines,
                max_bytes: query.max_bytes,
                if_none_match_sha256: query.if_none_match_sha256,
            },
        )
        .await?;
    ensure_plain_result(&result)?;

    let text = match &result.body {
        ReadTextBody::Content(content) => ReadTextOut::Content(ReadContentOut {
            content: content.content.clone(),
            content_sha256: result.content_sha256.clone(),
            byte_len: result.byte_len,
            line_count: result.line_count,
            start_line: content.start_line,
            end_line: content.end_line,
            returned_lines: content.returned_lines,
            truncated: content.truncated,
            next_start_line: content.next_start_line,
        }),
        ReadTextBody::Unchanged => ReadTextOut::Unchanged(ReadUnchangedOut {
            unchanged: true,
            content_returned: false,
            content_sha256: result.content_sha256.clone(),
            byte_len: result.byte_len,
            line_count: result.line_count,
        }),
        ReadTextBody::Encrypted(_) => return Err(encrypted_text_error()),
    };
    Ok(Json(ReadResponse {
        node: NodeOut::from(&result.node),
        text,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Replaces the complete plain-text content.
#[schema(example = serde_json::json!({
    "content": "# Updated document\n",
    "expected_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
}))]
pub(crate) struct ReplaceBody {
    /// Complete replacement UTF-8 content.
    content: String,
    /// Optional optimistic guard copied from the latest read response.
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[utoipa::path(
    put,
    path = "/api/v2/spaces/{space_id}/text/{node_id}",
    tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = ReplaceBody,
    responses((status = 200, description = "Replace plain text", body = TextMutationResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn replace(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReplaceBody>,
) -> Result<Json<TextMutationResponse>, ApiError> {
    let current_sha = guarded_plain_text_sha(
        &state,
        caller.account_id(),
        space_id,
        node_id,
        body.expected_sha256.as_deref(),
    )
    .await?;
    let view = state
        .files
        .write_text(
            caller.account_id(),
            space_id,
            WriteText {
                target: WriteTarget::Existing { node_id },
                body: WriteTextBody::Plain(body.content),
                expected_sha256: Some(current_sha),
            },
        )
        .await?;
    Ok(Json(text_mutation_response(&view)))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Appends UTF-8 content to an existing plain-text node.
#[schema(example = serde_json::json!({
    "content": "Next entry",
    "ensure_newline": true,
    "expected_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
}))]
pub(crate) struct AppendBody {
    /// UTF-8 content to append.
    content: String,
    /// Insert one line break before appending when the existing content is non-empty and lacks one.
    #[schema(default = false)]
    #[serde(default)]
    ensure_newline: bool,
    /// Optional optimistic guard copied from the latest read response.
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/text/{node_id}/append",
    tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = AppendBody,
    responses((status = 200, description = "Append plain text", body = TextMutationResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn append(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AppendBody>,
) -> Result<Json<TextMutationResponse>, ApiError> {
    let view = state
        .files
        .append_text(
            caller.account_id(),
            space_id,
            AppendText {
                target: WriteTarget::Existing { node_id },
                content: body.content,
                expected_sha256: body.expected_sha256,
                ensure_newline: body.ensure_newline,
            },
        )
        .await?;
    Ok(Json(text_mutation_response(&view)))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Applies one or more exact string replacements atomically.
#[schema(example = serde_json::json!({
    "edits": [{
        "old_text": "draft",
        "new_text": "published",
        "mode": "unique",
        "expected_count": 1
    }],
    "expected_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
}))]
pub(crate) struct PatchBody {
    /// Ordered exact replacements applied as one mutation.
    edits: Vec<PatchEditBody>,
    /// Optional optimistic guard copied from the latest read response.
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PatchEditBody {
    /// Exact source text to match.
    old_text: String,
    /// Replacement text.
    new_text: String,
    /// Match selection: `unique` (default), `first`, or `all`.
    #[schema(default = "unique", examples("unique", "first", "all"))]
    #[serde(default)]
    mode: Option<String>,
    /// Optional assertion for the number of matches before mutation.
    #[serde(default)]
    expected_count: Option<usize>,
}

#[utoipa::path(
    patch,
    path = "/api/v2/spaces/{space_id}/text/{node_id}",
    tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = PatchBody,
    responses((status = 200, description = "Apply exact text replacements", body = TextEditResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn patch(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchBody>,
) -> Result<Json<TextEditResponse>, ApiError> {
    let edits = body
        .edits
        .into_iter()
        .map(|edit| {
            Ok(ServiceEdit {
                old_text: edit.old_text,
                new_text: edit.new_text,
                mode: parse_patch_mode(edit.mode.as_deref())?,
                expected_count: edit.expected_count,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let result = state
        .files
        .patch_text(
            caller.account_id(),
            space_id,
            PatchText {
                node_id,
                edits,
                expected_sha256: body.expected_sha256,
            },
        )
        .await?;
    Ok(Json(text_edit_response(&result)))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Applies one or more 1-based line edits atomically.
#[schema(example = serde_json::json!({
    "edits": [{
        "op": "replace_lines",
        "start_line": 2,
        "end_line": 4,
        "content": "replacement\n"
    }],
    "expected_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
}))]
pub(crate) struct LineEditBody {
    /// Ordered line operations applied as one mutation.
    edits: Vec<LineEditItem>,
    /// Optional optimistic guard copied from the latest read response.
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LineEditItem {
    /// `insert_before_line`, `insert_after_line`, `replace_lines`, or `delete_lines`.
    #[schema(examples(
        "insert_before_line",
        "insert_after_line",
        "replace_lines",
        "delete_lines"
    ))]
    op: String,
    /// Required by insert operations. Lines are 1-based.
    #[serde(default)]
    line: Option<i64>,
    /// Required by replace and delete operations. Lines are 1-based and inclusive.
    #[serde(default)]
    start_line: Option<i64>,
    /// Required by replace and delete operations. Lines are 1-based and inclusive.
    #[serde(default)]
    end_line: Option<i64>,
    /// Required by insert and replace operations.
    #[serde(default)]
    content: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/text/{node_id}/edit",
    tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = LineEditBody,
    responses((status = 200, description = "Apply line-based edits", body = TextEditResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn edit(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<LineEditBody>,
) -> Result<Json<TextEditResponse>, ApiError> {
    let edits = body
        .edits
        .into_iter()
        .map(parse_line_edit)
        .collect::<Result<Vec<_>, ApiError>>()?;
    let result = state
        .files
        .edit_text(
            caller.account_id(),
            space_id,
            EditText {
                node_id,
                edits,
                expected_sha256: body.expected_sha256,
            },
        )
        .await?;
    Ok(Json(text_edit_response(&result)))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TextMutationResponse {
    node: NodeOut,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TextEditResponse {
    node: NodeOut,
    content_sha256: String,
    previous_sha256: String,
    byte_len: i64,
    line_count: i32,
    edits_applied: usize,
    diff: String,
    updated_at: DateTime<Utc>,
}

fn text_mutation_response(view: &TextView) -> TextMutationResponse {
    TextMutationResponse {
        node: NodeOut::from(&view.node),
        content_sha256: view.text.content_sha256.clone(),
        byte_len: view.text.byte_len,
        line_count: view.text.line_count,
        updated_at: view.text.updated_at,
    }
}

fn text_edit_response(result: &PatchResult) -> TextEditResponse {
    TextEditResponse {
        node: NodeOut::from(&result.node),
        content_sha256: result.text.content_sha256.clone(),
        previous_sha256: result.previous_sha256.clone(),
        byte_len: result.text.byte_len,
        line_count: result.text.line_count,
        edits_applied: result.edits_applied,
        diff: result.diff.clone(),
        updated_at: result.text.updated_at,
    }
}

fn ensure_plain_result(result: &ReadResult) -> Result<(), ApiError> {
    if result.storage_format == TextStorageFormat::Encrypted {
        return Err(encrypted_text_error());
    }
    Ok(())
}

fn encrypted_text_error() -> ApiError {
    ApiError::from(ServiceError::InvalidInput(
        ENCRYPTED_TEXT_MESSAGE.to_owned(),
    ))
}

fn parse_patch_mode(raw: Option<&str>) -> Result<PatchMode, ApiError> {
    PatchMode::parse(raw.unwrap_or("unique"))
        .ok_or_else(|| ApiError::invalid_field("mode must be 'unique', 'first', or 'all'"))
}

fn parse_line_edit(input: LineEditItem) -> Result<LineEdit, ApiError> {
    match input.op.as_str() {
        "insert_before_line" => Ok(LineEdit::InsertBefore {
            line: required(input.line, "line")?,
            content: required(input.content, "content")?,
        }),
        "insert_after_line" => Ok(LineEdit::InsertAfter {
            line: required(input.line, "line")?,
            content: required(input.content, "content")?,
        }),
        "replace_lines" => Ok(LineEdit::ReplaceLines {
            start_line: required(input.start_line, "start_line")?,
            end_line: required(input.end_line, "end_line")?,
            content: required(input.content, "content")?,
        }),
        "delete_lines" => Ok(LineEdit::DeleteLines {
            start_line: required(input.start_line, "start_line")?,
            end_line: required(input.end_line, "end_line")?,
        }),
        _ => Err(ApiError::invalid_field(
            "op must be insert_before_line, insert_after_line, replace_lines, or delete_lines",
        )),
    }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ApiError> {
    value.ok_or_else(|| ApiError::invalid_field(format!("{field} is required")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_edit_inputs_require_operation_fields() {
        let error = parse_line_edit(LineEditItem {
            op: "replace_lines".to_owned(),
            line: None,
            start_line: Some(1),
            end_line: None,
            content: Some("replacement".to_owned()),
        })
        .expect_err("missing end_line must fail");

        let response = axum::response::IntoResponse::into_response(error);
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn patch_modes_match_agent_mcp_contract() {
        for mode in [None, Some("unique"), Some("first"), Some("all")] {
            assert!(parse_patch_mode(mode).is_ok());
        }
        assert!(parse_patch_mode(Some("replace")).is_err());
    }
}
