//! Create commands: `mkdir` (folder) and `touch`/`write-create` (document).
//!
//! Both run in one transaction that re-checks every create invariant — parent is
//! a live folder, resulting depth ≤ 5, parent fanout < 200, workspace node count
//! < 10000, sibling-name unique (documents also: document count < 5000, byte
//! budget) — then inserts the node (and the `documents` row for a document) with
//! attribution = the caller.

use notegate_core::limits::{self, Limits};
use notegate_core::{Error, Result};
use notegate_model::{Document, Node};
use notegate_service::files::StoredContent;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{DOCUMENT_COLUMNS, DocumentRow, NODE_COLUMNS, NodeRow};
use super::checks;

/// Insert a folder under `parent_id`, attributing it to `created_by`.
pub async fn insert_folder(
    pool: &PgPool,
    workspace_id: Uuid,
    parent_id: Uuid,
    name: &str,
    created_by: Uuid,
    caps: Limits,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_workspace(&mut tx, workspace_id).await?;
    prepare_create(&mut tx, workspace_id, parent_id, name, caps).await?;

    let row = sqlx::query_as::<_, NodeRow>(&format!(
        "INSERT INTO nodes (workspace_id, parent_id, name, kind, created_by, updated_by) \
         VALUES ($1, $2, $3, 'folder', $4, $4) RETURNING {NODE_COLUMNS}"
    ))
    .bind(workspace_id)
    .bind(parent_id)
    .bind(name)
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_constraint_error)?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}

/// Insert a document node + its `documents` row, attributing both to
/// `created_by`. `content` carries the pre-computed metrics from the service.
pub async fn insert_document(
    pool: &PgPool,
    workspace_id: Uuid,
    parent_id: Uuid,
    name: &str,
    content: &StoredContent,
    created_by: Uuid,
    caps: Limits,
) -> Result<(Node, Document)> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_workspace(&mut tx, workspace_id).await?;
    prepare_create(&mut tx, workspace_id, parent_id, name, caps).await?;
    checks::require_document_budget(&mut tx, workspace_id, caps).await?;
    checks::require_byte_budget(&mut tx, workspace_id, 0, i64::from(content.byte_len), caps)
        .await?;

    let node_row = sqlx::query_as::<_, NodeRow>(&format!(
        "INSERT INTO nodes (workspace_id, parent_id, name, kind, created_by, updated_by) \
         VALUES ($1, $2, $3, 'document', $4, $4) RETURNING {NODE_COLUMNS}"
    ))
    .bind(workspace_id)
    .bind(parent_id)
    .bind(name)
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_constraint_error)?;

    let doc_row = sqlx::query_as::<_, DocumentRow>(&format!(
        "INSERT INTO documents \
            (node_id, workspace_id, content_md, content_sha256, byte_len, line_count, \
             created_by, updated_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7) RETURNING {DOCUMENT_COLUMNS}"
    ))
    .bind(node_row.id)
    .bind(workspace_id)
    .bind(&content.content_md)
    .bind(&content.content_sha256)
    .bind(content.byte_len)
    .bind(content.line_count)
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_constraint_error)?;

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok((node_row.into_node()?, Document::from(doc_row)))
}

/// Shared in-tx create pre-checks: parent live folder, depth, sibling-unique,
/// fanout, and workspace node budget.
async fn prepare_create(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    parent_id: Uuid,
    name: &str,
    caps: Limits,
) -> Result<()> {
    checks::require_live_folder(tx, workspace_id, parent_id).await?;

    let parent_depth = checks::node_depth(tx, workspace_id, parent_id).await?;
    if parent_depth + 1 > limits::MAX_PATH_DEPTH {
        return Err(Error::validation(format!(
            "path depth would exceed the maximum of {}",
            limits::MAX_PATH_DEPTH
        )));
    }

    checks::require_sibling_unique(tx, workspace_id, parent_id, name, None).await?;
    checks::require_fanout(tx, workspace_id, parent_id, caps).await?;
    checks::require_node_budget(tx, workspace_id, caps).await?;
    Ok(())
}
