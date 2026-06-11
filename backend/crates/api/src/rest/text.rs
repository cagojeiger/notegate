//! Text category: read / replace / patch UTF-8 text content.
//!
//! `GET /text/{node_id}` (bounded range + conditional read), `PUT` to
//! replace the whole text, and `PATCH` to apply exact targeted edits. All
//! delegate to the files service (no live permission ⇒ 404, insufficient
//! permission ⇒ 403; write and patch require write permission).

use axum::extract::{Extension, Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::rest::dto::{AccountRef, NodeRef};
use crate::state::AppState;

use notegate_service::files::{
    Edit as ServiceEdit, NodeView, PatchResult, PatchText, ReadResult, ReadText, TextView,
    WriteTarget, WriteText,
};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/v1/spaces/{space_id}/text/{node_id}",
        get(read).put(replace).patch(patch),
    )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReadQuery {
    #[serde(default)]
    start_line: Option<i64>,
    #[serde(default)]
    max_lines: Option<i64>,
    #[serde(default)]
    max_bytes: Option<usize>,
    #[serde(default)]
    if_none_match_sha256: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ReadResponse {
    node: NodeRef,
    text: ReadTextOut,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum ReadTextOut {
    Content(ReadContentOut),
    Unchanged(ReadUnchangedOut),
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ReadContentOut {
    node_id: Uuid,
    content: String,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
    start_line: i64,
    end_line: i64,
    returned_lines: i64,
    truncated: bool,
    next_start_line: Option<i64>,
    updated_by: AccountRef,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ReadUnchangedOut {
    node_id: Uuid,
    unchanged: bool,
    content_returned: bool,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/text/{node_id}",
    tag = "text",
    params(
        ("space_id" = Uuid, Path),
        ("node_id" = Uuid, Path),
        ("start_line" = Option<i64>, Query, description = "1-based first line to return"),
        ("max_lines" = Option<i64>, Query, description = "Maximum lines to return"),
        ("max_bytes" = Option<usize>, Query, description = "Maximum bytes to return"),
        ("if_none_match_sha256" = Option<String>, Query, description = "Conditional read content hash"),
    ),
    responses((status = 200, description = "Read text", body = ReadResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn read(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
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

    let updated_by = self_updated_by(&state, &result.node).await?;
    let text = read_text_out(&result, updated_by);
    Ok(Json(ReadResponse {
        node: NodeRef::from(&result.node),
        text,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct ReplaceBody {
    content: String,
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[utoipa::path(
    put,
    path = "/api/v1/spaces/{space_id}/text/{node_id}",
    tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = ReplaceBody,
    responses((status = 200, description = "Replace text", body = TextResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn replace(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReplaceBody>,
) -> Result<Json<TextResponse>, ApiError> {
    let view = state
        .files
        .write_text(
            caller.account_id(),
            space_id,
            WriteText {
                target: WriteTarget::Existing { node_id },
                content: body.content,
                expected_sha256: body.expected_sha256,
            },
        )
        .await?;
    let updated_by = self_updated_by(&state, &view.node).await?;
    Ok(Json(text_response(&view, updated_by)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct Edit {
    old_text: String,
    new_text: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct PatchBody {
    edits: Vec<Edit>,
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TextResponse {
    node: NodeRef,
    text: TextMetaOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TextMetaOut {
    node_id: Uuid,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
    updated_by: AccountRef,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PatchResponse {
    node: NodeRef,
    text: PatchTextOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PatchTextOut {
    node_id: Uuid,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
    previous_sha256: String,
    edits_applied: usize,
    diff: String,
    updated_by: AccountRef,
    updated_at: DateTime<Utc>,
}

#[utoipa::path(
    patch,
    path = "/api/v1/spaces/{space_id}/text/{node_id}",
    tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = PatchBody,
    responses((status = 200, description = "Patch text", body = PatchResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn patch(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchBody>,
) -> Result<Json<PatchResponse>, ApiError> {
    let edits = body
        .edits
        .into_iter()
        .map(|edit| ServiceEdit {
            old_text: edit.old_text,
            new_text: edit.new_text,
        })
        .collect();

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
    let updated_by = self_updated_by(&state, &result.node).await?;
    Ok(Json(patch_response(&result, updated_by)))
}

/// Resolve the `updated_by` account ref for a node view.
async fn self_updated_by(state: &AppState, view: &NodeView) -> Result<AccountRef, ApiError> {
    let refs = state
        .accounts
        .find_account_refs(&[view.node.updated_by_account_id])
        .await?;
    Ok(AccountRef::resolve(view.node.updated_by_account_id, &refs))
}

/// Build the `text` object for a read response, covering both the full-content
/// slice and the `unchanged` (conditional-read hit) shapes.
fn read_text_out(result: &ReadResult, updated_by: AccountRef) -> ReadTextOut {
    match &result.content {
        None => ReadTextOut::Unchanged(ReadUnchangedOut {
            node_id: result.node.node.id,
            unchanged: true,
            content_returned: false,
            content_sha256: result.content_sha256.clone(),
            byte_len: result.byte_len,
            line_count: result.line_count,
        }),
        Some(content) => ReadTextOut::Content(ReadContentOut {
            node_id: result.node.node.id,
            content: content.content.clone(),
            content_sha256: result.content_sha256.clone(),
            byte_len: result.byte_len,
            line_count: result.line_count,
            start_line: content.start_line,
            end_line: content.end_line,
            returned_lines: content.returned_lines,
            truncated: content.truncated,
            next_start_line: content.next_start_line,
            updated_by,
            updated_at: result.node.node.updated_at,
        }),
    }
}

/// Build the response returned after a successful replace.
fn text_response(view: &TextView, updated_by: AccountRef) -> TextResponse {
    TextResponse {
        node: NodeRef::from(&view.node),
        text: TextMetaOut {
            node_id: view.text.node_id,
            content_sha256: view.text.content_sha256.clone(),
            byte_len: view.text.byte_len,
            line_count: view.text.line_count,
            updated_by,
            updated_at: view.text.updated_at,
        },
    }
}

/// Build the response after a successful patch (new metrics + previous hash).
fn patch_response(result: &PatchResult, updated_by: AccountRef) -> PatchResponse {
    PatchResponse {
        node: NodeRef::from(&result.node),
        text: PatchTextOut {
            node_id: result.text.node_id,
            content_sha256: result.text.content_sha256.clone(),
            byte_len: result.text.byte_len,
            line_count: result.text.line_count,
            previous_sha256: result.previous_sha256.clone(),
            edits_applied: result.edits_applied,
            diff: result.diff.clone(),
            updated_by,
            updated_at: result.text.updated_at,
        },
    }
}
