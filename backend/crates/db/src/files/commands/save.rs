//! Save command: replace a document's content + metrics (`write`/`patch`).
//!
//! Runs in one transaction: re-reads the document's current byte length, enforces
//! the workspace byte budget for the replacement, updates `documents` content +
//! metrics + attribution, and bumps the node's `updated_by`/`updated_at`.

use notegate_core::{Error, Result};
use notegate_model::{Document, Node};
use notegate_service::files::StoredContent;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{DOCUMENT_COLUMNS, DocumentRow, NODE_COLUMNS, NodeRow};
use super::checks;

/// Replace a live document's content + metrics, attributing the update to
/// `updated_by` on both the document and its node.
pub async fn save_document_content(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
    content: &StoredContent,
    expected_sha256: Option<&str>,
    updated_by: Uuid,
) -> Result<(Node, Document)> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    // Current byte length/hash (for budget delta + optimistic guard); the
    // document row is locked so `expected_sha256` is compared atomically with
    // the following update.
    let previous: Option<(i64, String)> = sqlx::query_as(
        "SELECT d.byte_len::bigint, d.content_sha256 FROM documents d \
         JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
         WHERE d.workspace_id = $1 AND d.node_id = $2 AND n.deleted_at IS NULL \
         FOR UPDATE OF d",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let (previous_bytes, previous_sha256) =
        previous.ok_or_else(|| Error::not_found("document not found"))?;
    if let Some(expected) = expected_sha256
        && expected != previous_sha256
    {
        return Err(Error::conflict(
            "expected_sha256 does not match the current document; read it again",
        ));
    }

    checks::require_byte_budget(
        &mut tx,
        workspace_id,
        previous_bytes,
        i64::from(content.byte_len),
    )
    .await?;

    let doc_row = sqlx::query_as::<_, DocumentRow>(&format!(
        "UPDATE documents \
         SET content_md = $3, content_sha256 = $4, byte_len = $5, line_count = $6, \
             updated_by = $7, updated_at = now() \
         WHERE workspace_id = $1 AND node_id = $2 RETURNING {DOCUMENT_COLUMNS}"
    ))
    .bind(workspace_id)
    .bind(node_id)
    .bind(&content.content_md)
    .bind(&content.content_sha256)
    .bind(content.byte_len)
    .bind(content.line_count)
    .bind(updated_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_constraint_error)?;

    let node_row = sqlx::query_as::<_, NodeRow>(&format!(
        "UPDATE nodes SET updated_by = $3, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2 RETURNING {NODE_COLUMNS}"
    ))
    .bind(workspace_id)
    .bind(node_id)
    .bind(updated_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok((node_row.into_node()?, Document::from(doc_row)))
}
