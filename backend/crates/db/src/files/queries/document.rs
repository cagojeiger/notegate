//! Document reads: load a live document (node + content), and workspace-level
//! document count / total byte sum used by the in-tx capacity checks.

use notegate_core::Result;
use notegate_model::{Document, Node};
use notegate_service::files::DocumentStats;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::map_sqlx_error;
use super::super::rows::{DOCUMENT_COLUMNS, DocumentRow, NODE_COLUMNS, NodeRow};
use crate::to_usize;

/// Load live document metrics without the Markdown body.
pub async fn document_stats(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
) -> Result<Option<DocumentStats>> {
    let row: Option<(String, i32, i32)> = sqlx::query_as(
        "SELECT d.content_sha256, d.byte_len, d.line_count FROM documents d \
         JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
         WHERE d.workspace_id = $1 AND d.node_id = $2 AND n.deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(
        row.map(|(content_sha256, byte_len, line_count)| DocumentStats {
            content_sha256,
            byte_len,
            line_count,
        }),
    )
}

/// Load a live document (its node + content) by node id, or `None` when the node
/// is missing, soft-deleted, or a folder.
pub async fn find_document(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
) -> Result<Option<(Node, Document)>> {
    let node_row = sqlx::query_as::<_, NodeRow>(&format!(
        "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL AND kind = 'document'"
    ))
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    let Some(node_row) = node_row else {
        return Ok(None);
    };

    let doc_row = sqlx::query_as::<_, DocumentRow>(&format!(
        "SELECT {DOCUMENT_COLUMNS} FROM documents \
         WHERE workspace_id = $1 AND node_id = $2"
    ))
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    match doc_row {
        Some(doc_row) => Ok(Some((node_row.into_node()?, Document::from(doc_row)))),
        None => Ok(None),
    }
}

/// Count of live documents in a workspace (joins `documents` to live nodes).
pub async fn count_live_documents(pool: &PgPool, workspace_id: Uuid) -> Result<usize> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM documents d \
         JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
         WHERE d.workspace_id = $1 AND n.deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    to_usize(count, "document")
}

/// Sum of `byte_len` over the workspace's live documents.
pub async fn sum_live_document_bytes(pool: &PgPool, workspace_id: Uuid) -> Result<usize> {
    let total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(d.byte_len), 0)::bigint FROM documents d \
         JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
         WHERE d.workspace_id = $1 AND n.deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    to_usize(total, "document byte")
}
