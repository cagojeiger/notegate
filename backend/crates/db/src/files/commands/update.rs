//! Update-metadata command (`PATCH /nodes/{id}`): rename and/or reorder a node
//! in place, without changing its parent.
//!
//! Runs in one transaction: the node must exist and be live; the root cannot be
//! renamed; a rename re-checks sibling-name uniqueness at the current parent. Only
//! the supplied fields change (`NULL` leaves a column unchanged via `COALESCE`),
//! plus attribution.

use notegate_core::{Error, Result};
use notegate_model::Node;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{NODE_COLUMNS, NodeRow};
use super::checks;

/// Update `node_id`'s `name` and/or `sort_order` in place, attributing the change
/// to `updated_by`. `None` fields are left unchanged.
pub async fn update_node_metadata(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
    new_name: Option<&str>,
    new_sort_order: Option<i32>,
    updated_by: Uuid,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    let node = checks::live_node(&mut tx, workspace_id, node_id)
        .await?
        .ok_or_else(|| Error::not_found("node not found"))?;

    if let Some(name) = new_name {
        // The root node (no parent) cannot be renamed.
        let Some(parent_id) = node.parent_id else {
            return Err(Error::conflict("cannot rename the root node"));
        };
        checks::require_sibling_unique(&mut tx, workspace_id, parent_id, name, Some(node_id))
            .await?;
    }

    let row = sqlx::query_as::<_, NodeRow>(&format!(
        "UPDATE nodes \
         SET name = COALESCE($3, name), \
             sort_order = COALESCE($4, sort_order), \
             updated_by = $5, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2 RETURNING {NODE_COLUMNS}"
    ))
    .bind(workspace_id)
    .bind(node_id)
    .bind(new_name)
    .bind(new_sort_order)
    .bind(updated_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_constraint_error)?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}
