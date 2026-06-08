//! Restore command (`restore`): un-soft-delete a node and the subtree that was
//! deleted with it.
//!
//! Runs in one transaction. Rejects the restore if any ancestor is still
//! soft-deleted (the parent chain would orphan the node — consensus rule), then
//! re-validates sibling-name uniqueness, destination fanout, and resulting depth
//! against the now-live parent. Clears `deleted_at`/`deleted_by` on the target
//! and every currently-deleted descendant reachable through deleted nodes, and
//! bumps the target's `updated_by`/`updated_at`.

use notegate_core::{Error, Result, limits};
use notegate_model::Node;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{NODE_COLUMNS, NodeRow};
use super::checks;

/// Restore the soft-deleted `node_id` (and its deleted subtree), attributing the
/// update to `restored_by`.
pub async fn restore_node(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
    restored_by: Uuid,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_workspace(&mut tx, workspace_id).await?;

    // The target must exist and be soft-deleted; the root is never deleted.
    let row: Option<(Option<Uuid>, String)> = sqlx::query_as(
        "SELECT parent_id, name FROM nodes \
         WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NOT NULL",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let Some((parent_id, name)) = row else {
        return Err(Error::not_found("deleted node not found"));
    };
    let parent_id = parent_id.ok_or_else(|| Error::validation("the root node is never deleted"))?;

    // Reject when any ancestor is still soft-deleted (walk up without the live
    // filter; the target's own deleted_at is excluded via depth > 0).
    let ancestor_deleted: bool = sqlx::query_scalar(
        "WITH RECURSIVE chain AS ( \
            SELECT id, parent_id, deleted_at, 0 AS depth \
            FROM nodes WHERE workspace_id = $1 AND id = $2 \
            UNION ALL \
            SELECT n.id, n.parent_id, n.deleted_at, c.depth + 1 \
            FROM nodes n JOIN chain c ON n.id = c.parent_id \
            WHERE n.workspace_id = $1 \
         ) \
         SELECT EXISTS (SELECT 1 FROM chain WHERE depth > 0 AND deleted_at IS NOT NULL)",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if ancestor_deleted {
        return Err(Error::validation(
            "an ancestor is still deleted; restore the ancestor folder first",
        ));
    }

    // Re-validate against the now-live parent.
    checks::require_sibling_unique(&mut tx, workspace_id, parent_id, &name, Some(node_id)).await?;
    checks::require_fanout(&mut tx, workspace_id, parent_id).await?;

    // Resulting depth: parent depth + 1 + the deleted subtree's relative depth.
    let parent_depth = checks::node_depth(&mut tx, workspace_id, parent_id).await?;
    let subtree_depth = deleted_subtree_relative_depth(&mut tx, workspace_id, node_id).await?;
    if parent_depth + 1 + subtree_depth > limits::MAX_PATH_DEPTH {
        return Err(Error::validation(format!(
            "restore would exceed the maximum path depth of {}",
            limits::MAX_PATH_DEPTH
        )));
    }

    require_restore_budget(&mut tx, workspace_id, node_id).await?;

    // Clear deletion on the target + every deleted descendant reachable through
    // deleted nodes (the subtree that was deleted with it).
    sqlx::query(
        "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NOT NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NOT NULL \
         ) \
         UPDATE nodes SET deleted_at = NULL, deleted_by = NULL \
         WHERE workspace_id = $1 AND id IN (SELECT id FROM subtree)",
    )
    .bind(workspace_id)
    .bind(node_id)
    .execute(&mut *tx)
    .await
    .map_err(map_constraint_error)?;

    let restored = sqlx::query_as::<_, NodeRow>(&format!(
        "UPDATE nodes SET updated_by = $3, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2 RETURNING {NODE_COLUMNS}"
    ))
    .bind(workspace_id)
    .bind(node_id)
    .bind(restored_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    tx.commit().await.map_err(map_sqlx_error)?;
    restored.into_node()
}

/// Enforce live workspace quotas for the deleted subtree that is about to become
/// live again.
async fn require_restore_budget(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    node_id: Uuid,
) -> Result<()> {
    let (restore_nodes, restore_documents, restore_bytes): (i64, i64, i64) = sqlx::query_as(
        "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NOT NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NOT NULL \
         ) \
         SELECT count(s.id)::bigint, \
                count(d.node_id)::bigint, \
                COALESCE(sum(d.byte_len), 0)::bigint \
         FROM subtree s \
         LEFT JOIN documents d ON d.workspace_id = $1 AND d.node_id = s.id",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let (live_nodes, live_documents, live_bytes): (i64, i64, i64) = sqlx::query_as(
        "SELECT \
            (SELECT count(*)::bigint FROM nodes WHERE workspace_id = $1 AND deleted_at IS NULL), \
            (SELECT count(*)::bigint FROM documents d \
             JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
             WHERE d.workspace_id = $1 AND n.deleted_at IS NULL), \
            (SELECT COALESCE(sum(d.byte_len), 0)::bigint FROM documents d \
             JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
             WHERE d.workspace_id = $1 AND n.deleted_at IS NULL)",
    )
    .bind(workspace_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    if live_nodes + restore_nodes > limits::WORKSPACE_MAX_NODES as i64 {
        return Err(Error::conflict(format!(
            "restore would exceed the workspace node limit of {}",
            limits::WORKSPACE_MAX_NODES
        )));
    }
    if live_documents + restore_documents > limits::WORKSPACE_MAX_DOCUMENTS as i64 {
        return Err(Error::conflict(format!(
            "restore would exceed the workspace document limit of {}",
            limits::WORKSPACE_MAX_DOCUMENTS
        )));
    }
    if live_bytes + restore_bytes > limits::WORKSPACE_MAX_DOCUMENT_BYTES as i64 {
        return Err(Error::conflict(format!(
            "restore would exceed the workspace document byte budget of {}",
            limits::WORKSPACE_MAX_DOCUMENT_BYTES
        )));
    }

    Ok(())
}

/// Maximum depth of the deleted subtree rooted at `node_id` relative to it (0 if
/// it has no deleted descendants). Mirrors the live variant but over deleted
/// rows, so restore validates the depth of what it is about to revive.
async fn deleted_subtree_relative_depth(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    node_id: Uuid,
) -> Result<usize> {
    let depth: i64 = sqlx::query_scalar(
        "WITH RECURSIVE subtree AS ( \
            SELECT id, 0 AS depth \
            FROM nodes WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NOT NULL \
            UNION ALL \
            SELECT n.id, s.depth + 1 \
            FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NOT NULL \
         ) \
         SELECT COALESCE(max(depth), 0)::bigint FROM subtree",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    usize::try_from(depth).map_err(|_error| Error::internal("negative depth count"))
}
