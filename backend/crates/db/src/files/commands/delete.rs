//! Soft-delete command (`rm`).
//!
//! Soft-deletes the node and its entire live subtree (folders are recursive) in
//! one transaction, setting `deleted_at`/`deleted_by`. The subtree size is
//! re-checked in-tx against `subtree_delete_max_nodes`; a larger subtree is
//! rejected so a synchronous delete never touches an unbounded number of rows.

use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::map_sqlx_error;
use super::checks;

/// Soft-delete `node_id` and its live subtree, attributing it to `deleted_by`.
pub async fn soft_delete_node(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
    deleted_by: Uuid,
) -> Result<DateTime<Utc>> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_workspace(&mut tx, workspace_id).await?;

    // Bound the synchronous delete by the live subtree size.
    let subtree: i64 = sqlx::query_scalar(
        "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT count(*) FROM subtree",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let subtree =
        usize::try_from(subtree).map_err(|_error| Error::internal("negative subtree count"))?;
    if subtree == 0 {
        return Err(Error::not_found("node not found"));
    }
    if subtree > limits::SUBTREE_DELETE_MAX_NODES {
        return Err(Error::conflict(format!(
            "subtree of {subtree} nodes exceeds the synchronous delete limit of {}",
            limits::SUBTREE_DELETE_MAX_NODES
        )));
    }

    let purge_after: DateTime<Utc> =
        sqlx::query_scalar("SELECT now() + ($1::bigint * interval '1 day')")
            .bind(limits::DELETED_NODE_RETENTION_DAYS)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

    // Soft-delete the whole live subtree in one statement.
    sqlx::query(
        "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         UPDATE nodes SET deleted_at = now(), deleted_by = $3, purge_after = $4 \
         WHERE workspace_id = $1 AND id IN (SELECT id FROM subtree)",
    )
    .bind(workspace_id)
    .bind(node_id)
    .bind(deleted_by)
    .bind(purge_after)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok(purge_after)
}
