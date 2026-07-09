//! Move/rename command (`mv`).
//!
//! O(1): this UPDATEs ONLY the moved node's `parent_id`/`name` (plus
//! attribution). Descendants are never rewritten — their paths are derived,
//! so they follow the moved node automatically. The transaction re-checks the
//! move invariants: destination is a live folder, the move is not into the node
//! itself or its own subtree, sibling-name is unique at the destination, and
//! the resulting subtree depth and destination fanout stay within limits.

use notegate_core::limits::{self, Limits};
use notegate_core::{Error, Result};
use notegate_model::Node;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{NODE_COLUMNS, NodeRow};
use super::checks;
use crate::file_change_events;

pub struct MoveNodeArgs<'a> {
    pub pool: &'a PgPool,
    pub space_id: Uuid,
    pub node_id: Uuid,
    pub new_parent_id: Uuid,
    pub new_name: Option<&'a str>,
    pub expected_parent_id: Option<Uuid>,
    pub updated_by: Uuid,
    pub caps: Limits,
}

/// Move/rename `node_id` to `new_parent_id` with optional `new_name`, attributing
/// the update to `updated_by`. Updates only the moved node's row.
pub async fn move_node(args: MoveNodeArgs<'_>) -> Result<Node> {
    let MoveNodeArgs {
        pool,
        space_id,
        node_id,
        new_parent_id,
        new_name,
        expected_parent_id,
        updated_by,
        caps,
    } = args;
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_space(&mut tx, space_id).await?;
    let caps = checks::effective_limits_for_locked_space(&mut tx, space_id, caps).await?;

    // The moved node must exist and be live; the root cannot be moved.
    let moved_row = sqlx::query_as::<_, NodeRow>(&format!(
        "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
         FOR UPDATE"
    ))
    .bind(space_id)
    .bind(node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::not_found("node not found"))?;
    let current_parent_id = moved_row.parent_id;
    let current_name = moved_row.name.clone();
    let moved_kind = moved_row.kind.clone();
    if current_parent_id.is_none() {
        return Err(Error::conflict("cannot move the root node"));
    }
    if let Some(expected_parent_id) = expected_parent_id
        && current_parent_id != Some(expected_parent_id)
    {
        return Err(Error::conflict(
            "expected_parent_id does not match the node's current parent; refresh and retry",
        ));
    }
    let final_name = new_name.unwrap_or(&current_name);
    if current_parent_id == Some(new_parent_id) && final_name == current_name {
        tx.commit().await.map_err(map_sqlx_error)?;
        return moved_row.into_node();
    }

    // Destination must be a live folder.
    checks::require_live_folder(&mut tx, space_id, new_parent_id).await?;

    // Cannot move into self or own descendant (recursive subtree membership).
    let into_subtree: bool = sqlx::query_scalar(
        "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT EXISTS (SELECT 1 FROM subtree WHERE id = $3)",
    )
    .bind(space_id)
    .bind(node_id)
    .bind(new_parent_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if into_subtree {
        return Err(Error::conflict(
            "cannot move a node into itself or its descendant",
        ));
    }

    // Sibling-name unique at destination (ignoring the node itself).
    checks::require_sibling_unique(&mut tx, space_id, new_parent_id, final_name, Some(node_id))
        .await?;

    // Resulting subtree depth: dest depth + 1 (the moved node) + its subtree depth.
    let dest_depth = checks::node_depth(&mut tx, space_id, new_parent_id).await?;
    let subtree_depth = checks::subtree_relative_depth(&mut tx, space_id, node_id).await?;
    if dest_depth + 1 + subtree_depth > limits::MAX_PATH_DEPTH {
        return Err(Error::conflict(format!(
            "move would exceed the maximum path depth of {}",
            limits::MAX_PATH_DEPTH
        )));
    }

    // Destination fanout, only when actually changing parent.
    if current_parent_id != Some(new_parent_id) {
        checks::require_fanout(&mut tx, space_id, new_parent_id, caps).await?;
    }

    let row = sqlx::query_as::<_, NodeRow>(&format!(
            "UPDATE nodes SET parent_id = $3, name = $4, updated_by_account_id = $5, updated_at = now() \
         WHERE space_id = $1 AND id = $2 RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(node_id)
        .bind(new_parent_id)
        .bind(final_name)
        .bind(updated_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    file_change_events::record(
        &mut tx,
        file_change_events::context(updated_by, space_id),
        Some(node_id),
        "item.move",
        json!({
            "item_kind": moved_kind,
            "parent_node_id_before": current_parent_id,
            "parent_node_id_after": new_parent_id,
            "name_changed": final_name != current_name,
        }),
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}
