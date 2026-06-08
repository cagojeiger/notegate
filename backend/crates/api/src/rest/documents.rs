//! Documents category: read / replace / patch Markdown content.
//!
//! `GET /documents/{node_id}` (bounded range + conditional read), `PUT` to
//! replace the whole document, and `PATCH` to apply exact targeted edits. All
//! delegate to the files service (no live role ⇒ 404, lesser role ⇒ 403; write
//! and patch require `editor`).

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
    DocumentView, Edit as ServiceEdit, NodeView, PatchDocument, PatchResult, ReadDocument,
    ReadResult, WriteDocument, WriteTarget,
};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/v1/workspaces/{workspace_id}/documents/{node_id}",
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
    document: ReadDocumentOut,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum ReadDocumentOut {
    Content(ReadContentOut),
    Unchanged(ReadUnchangedOut),
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ReadContentOut {
    node_id: Uuid,
    content_md: String,
    content_sha256: String,
    byte_len: i32,
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
    byte_len: i32,
    line_count: i32,
}

#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{workspace_id}/documents/{node_id}",
    tag = "documents",
    params(("workspace_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses((status = 200, description = "Read document", body = ReadResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn read(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
    Path((workspace_id, node_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ReadQuery>,
) -> Result<Json<ReadResponse>, ApiError> {
    let result = state
        .files
        .read_document(
            caller.account_id(),
            workspace_id,
            ReadDocument {
                node_id,
                start_line: query.start_line,
                max_lines: query.max_lines,
                max_bytes: query.max_bytes,
                if_none_match_sha256: query.if_none_match_sha256,
            },
        )
        .await?;

    let updated_by = self_updated_by(&state, &result.node).await?;
    let document = read_document_out(&result, updated_by);
    Ok(Json(ReadResponse {
        node: NodeRef::from(&result.node),
        document,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct ReplaceBody {
    content_md: String,
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[utoipa::path(
    put,
    path = "/api/v1/workspaces/{workspace_id}/documents/{node_id}",
    tag = "documents",
    params(("workspace_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = ReplaceBody,
    responses((status = 200, description = "Replace document", body = DocumentResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn replace(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
    Path((workspace_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReplaceBody>,
) -> Result<Json<DocumentResponse>, ApiError> {
    let view = state
        .files
        .write_document(
            caller.account_id(),
            workspace_id,
            WriteDocument {
                target: WriteTarget::Existing { node_id },
                content_md: body.content_md,
                expected_sha256: body.expected_sha256,
            },
        )
        .await?;
    let updated_by = self_updated_by(&state, &view.node).await?;
    Ok(Json(document_response(&view, updated_by)))
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
pub(crate) struct DocumentResponse {
    node: NodeRef,
    document: DocumentMetaOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DocumentMetaOut {
    node_id: Uuid,
    content_sha256: String,
    byte_len: i32,
    line_count: i32,
    updated_by: AccountRef,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PatchResponse {
    node: NodeRef,
    document: PatchDocumentOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PatchDocumentOut {
    node_id: Uuid,
    content_sha256: String,
    byte_len: i32,
    line_count: i32,
    previous_sha256: String,
    edits_applied: usize,
    diff: String,
    updated_by: AccountRef,
    updated_at: DateTime<Utc>,
}

#[utoipa::path(
    patch,
    path = "/api/v1/workspaces/{workspace_id}/documents/{node_id}",
    tag = "documents",
    params(("workspace_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = PatchBody,
    responses((status = 200, description = "Patch document", body = PatchResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn patch(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
    Path((workspace_id, node_id)): Path<(Uuid, Uuid)>,
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
        .patch_document(
            caller.account_id(),
            workspace_id,
            PatchDocument {
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
        .find_account_refs(&[view.node.updated_by])
        .await?;
    Ok(AccountRef::resolve(view.node.updated_by, &refs))
}

/// Build the `document` object for a read response, covering both the full-content
/// slice and the `unchanged` (conditional-read hit) shapes.
fn read_document_out(result: &ReadResult, updated_by: AccountRef) -> ReadDocumentOut {
    match &result.content {
        None => ReadDocumentOut::Unchanged(ReadUnchangedOut {
            node_id: result.node.node.id,
            unchanged: true,
            content_returned: false,
            content_sha256: result.content_sha256.clone(),
            byte_len: result.byte_len,
            line_count: result.line_count,
        }),
        Some(content) => ReadDocumentOut::Content(ReadContentOut {
            node_id: result.node.node.id,
            content_md: content.content_md.clone(),
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
fn document_response(view: &DocumentView, updated_by: AccountRef) -> DocumentResponse {
    DocumentResponse {
        node: NodeRef::from(&view.node),
        document: DocumentMetaOut {
            node_id: view.document.node_id,
            content_sha256: view.document.content_sha256.clone(),
            byte_len: view.document.byte_len,
            line_count: view.document.line_count,
            updated_by,
            updated_at: view.document.updated_at,
        },
    }
}

/// Build the response after a successful patch (new metrics + previous hash).
fn patch_response(result: &PatchResult, updated_by: AccountRef) -> PatchResponse {
    PatchResponse {
        node: NodeRef::from(&result.node),
        document: PatchDocumentOut {
            node_id: result.document.node_id,
            content_sha256: result.document.content_sha256.clone(),
            byte_len: result.document.byte_len,
            line_count: result.document.line_count,
            previous_sha256: result.previous_sha256.clone(),
            edits_applied: result.edits_applied,
            diff: result.diff.clone(),
            updated_by,
            updated_at: result.document.updated_at,
        },
    }
}
