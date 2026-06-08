//! Move/rename command (`mv`).
//!
//! O(1): this UPDATEs ONLY the moved node's `parent_id`/`name` (plus
//! attribution). Descendants are never rewritten — their paths are derived,
//! so they follow the moved node automatically. The transaction re-checks the
//! move invariants: destination is a live folder, the move is not into the node
//! itself or its own subtree, sibling-name is unique at the destination, the
//! resulting subtree depth ≤ 5, and the destination fanout < 200.

use notegate_core::{Error, Result, limits};
use notegate_model::Node;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{NODE_COLUMNS, NodeRow};
use super::checks;

/// Move/rename `node_id` to `new_parent_id` with optional `new_name`, attributing
/// the update to `updated_by`. Updates only the moved node's row.
pub async fn move_node(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
    new_parent_id: Uuid,
    new_name: Option<&str>,
    updated_by: Uuid,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    // The moved node must exist and be live; the root cannot be moved.
    let moved = checks::live_node(&mut tx, workspace_id, node_id)
        .await?
        .ok_or_else(|| Error::not_found("node not found"))?;
    if moved.parent_id.is_none() {
        return Err(Error::validation("cannot move the root node"));
    }
    let current_name: String =
        sqlx::query_scalar("SELECT name FROM nodes WHERE workspace_id = $1 AND id = $2")
            .bind(workspace_id)
            .bind(node_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
    let final_name = new_name.unwrap_or(&current_name);

    // Destination must be a live folder.
    checks::require_live_folder(&mut tx, workspace_id, new_parent_id).await?;

    // Cannot move into self or own descendant (recursive subtree membership).
    let into_subtree: bool = sqlx::query_scalar(
        "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT EXISTS (SELECT 1 FROM subtree WHERE id = $3)",
    )
    .bind(workspace_id)
    .bind(node_id)
    .bind(new_parent_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if into_subtree {
        return Err(Error::validation(
            "cannot move a node into itself or its descendant",
        ));
    }

    // Sibling-name unique at destination (ignoring the node itself).
    checks::require_sibling_unique(
        &mut tx,
        workspace_id,
        new_parent_id,
        final_name,
        Some(node_id),
    )
    .await?;

    // Resulting subtree depth: dest depth + 1 (the moved node) + its subtree depth.
    let dest_depth = checks::node_depth(&mut tx, workspace_id, new_parent_id).await?;
    let subtree_depth = checks::subtree_relative_depth(&mut tx, workspace_id, node_id).await?;
    if dest_depth + 1 + subtree_depth > limits::MAX_PATH_DEPTH {
        return Err(Error::validation(format!(
            "move would exceed the maximum path depth of {}",
            limits::MAX_PATH_DEPTH
        )));
    }

    // Destination fanout, only when actually changing parent.
    if moved.parent_id != Some(new_parent_id) {
        checks::require_fanout(&mut tx, workspace_id, new_parent_id).await?;
    }

    let row = sqlx::query_as::<_, NodeRow>(&format!(
        "UPDATE nodes SET parent_id = $3, name = $4, updated_by = $5, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2 RETURNING {NODE_COLUMNS}"
    ))
    .bind(workspace_id)
    .bind(node_id)
    .bind(new_parent_id)
    .bind(final_name)
    .bind(updated_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_constraint_error)?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}
