//! Documents category: read / replace / patch Markdown content.
//!
//! `GET /documents/{node_id}` (bounded range + conditional read), `PUT` to
//! replace the whole document, and `PATCH` to apply exact targeted edits. All
//! delegate to the files service (no live role ⇒ 404, lesser role ⇒ 403; write
//! and patch require `editor`).

use axum::extract::{Extension, Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
    document: Value,
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
    let document = read_document_json(&result, updated_by);
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
    responses((status = 200, description = "Replace document")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn replace(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
    Path((workspace_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReplaceBody>,
) -> Result<Json<Value>, ApiError> {
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
    Ok(Json(document_view_json(&view, updated_by)))
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

#[utoipa::path(
    patch,
    path = "/api/v1/workspaces/{workspace_id}/documents/{node_id}",
    tag = "documents",
    params(("workspace_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = PatchBody,
    responses((status = 200, description = "Patch document")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn patch(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
    Path((workspace_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchBody>,
) -> Result<Json<Value>, ApiError> {
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
    Ok(Json(patch_result_json(&result, updated_by)))
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
fn read_document_json(result: &ReadResult, updated_by: AccountRef) -> Value {
    match &result.content {
        None => json!({
            "node_id": result.node.node.id,
            "unchanged": true,
            "content_returned": false,
            "content_sha256": result.content_sha256,
            "byte_len": result.byte_len,
            "line_count": result.line_count,
        }),
        Some(content) => json!({
            "node_id": result.node.node.id,
            "content_md": content.content_md,
            "content_sha256": result.content_sha256,
            "byte_len": result.byte_len,
            "line_count": result.line_count,
            "start_line": content.start_line,
            "end_line": content.end_line,
            "returned_lines": content.returned_lines,
            "truncated": content.truncated,
            "next_start_line": content.next_start_line,
            "updated_by": account_ref_json(&updated_by),
            "updated_at": result.node.node.updated_at,
        }),
    }
}

/// Build the `document` object returned after a successful replace.
fn document_view_json(view: &DocumentView, updated_by: AccountRef) -> Value {
    json!({
        "node": NodeRef::from(&view.node),
        "document": {
            "node_id": view.document.node_id,
            "content_sha256": view.document.content_sha256,
            "byte_len": view.document.byte_len,
            "line_count": view.document.line_count,
            "updated_by": account_ref_json(&updated_by),
            "updated_at": view.document.updated_at,
        }
    })
}

/// Build the response after a successful patch (new metrics + previous hash).
fn patch_result_json(result: &PatchResult, updated_by: AccountRef) -> Value {
    json!({
        "node": NodeRef::from(&result.node),
        "document": {
            "node_id": result.document.node_id,
            "content_sha256": result.document.content_sha256,
            "byte_len": result.document.byte_len,
            "line_count": result.document.line_count,
            "previous_sha256": result.previous_sha256,
            "edits_applied": result.edits_applied,
            "diff": result.diff,
            "updated_by": account_ref_json(&updated_by),
            "updated_at": result.document.updated_at,
        }
    })
}

fn account_ref_json(account: &AccountRef) -> Value {
    json!({
        "id": account.id,
        "kind": account.kind,
        "display_name": account.display_name,
    })
}
