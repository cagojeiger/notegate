//! Update-metadata command (`PATCH /nodes/{id}`): rename and/or reorder a node
//! in place, without changing its parent.
//!
//! Runs in one transaction serialized by the space row: the node must exist
//! and be live; the root cannot be renamed; a rename re-checks sibling-name
//! uniqueness at the current parent. Only
//! the supplied fields change (`NULL` leaves a column unchanged via `COALESCE`),
//! plus attribution.

use notegate_core::{Error, Result};
use notegate_model::Node;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{NODE_COLUMNS, NodeRow};
use super::checks;
use crate::file_change_events;
use crate::files_repo::MetadataMutationKind;

/// Update `node_id`'s `name` and/or `sort_order` in place, attributing the change
/// to `updated_by`. `None` fields are left unchanged.
pub async fn update_node_metadata(
    pool: &PgPool,
    space_id: Uuid,
    node_id: Uuid,
    new_name: Option<&str>,
    new_sort_order: Option<i32>,
    updated_by: Uuid,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_space(&mut tx, space_id).await?;

    let current = sqlx::query_as::<_, NodeRow>(&format!(
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
    let node_kind = current.kind.clone();
    let parent_id = current.parent_id;
    let current_name = current.name.clone();
    let current_sort_order = current.sort_order;

    if new_name.is_some() && parent_id.is_none() {
        return Err(Error::conflict("cannot rename the root node"));
    }

    let name_changed = new_name.is_some_and(|name| name != current_name);
    let sort_order_changed =
        new_sort_order.is_some_and(|sort_order| sort_order != current_sort_order);
    if !name_changed && !sort_order_changed {
        tx.commit().await.map_err(map_sqlx_error)?;
        return current.into_node();
    }

    if let Some(name) = new_name
        && name_changed
    {
        let Some(parent_id) = parent_id else {
            return Err(Error::conflict("cannot rename the root node"));
        };
        checks::require_sibling_unique(&mut tx, space_id, parent_id, name, Some(node_id)).await?;
    }

    let row = sqlx::query_as::<_, NodeRow>(&format!(
        "UPDATE nodes \
         SET name = COALESCE($3, name), \
             sort_order = COALESCE($4, sort_order), \
             updated_by_account_id = $5, updated_at = now() \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING {NODE_COLUMNS}"
    ))
    .bind(space_id)
    .bind(node_id)
    .bind(new_name)
    .bind(new_sort_order)
    .bind(updated_by)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_constraint_error)?
    .ok_or_else(|| Error::not_found("node not found"))?;

    file_change_events::node_updated(
        &mut tx,
        file_change_events::context(updated_by, space_id),
        node_id,
        &node_kind,
        &row.name,
        name_changed,
        sort_order_changed,
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}

/// Replace `node_id`'s metadata object in place.
pub async fn replace_node_metadata(
    pool: &PgPool,
    space_id: Uuid,
    node_id: Uuid,
    metadata: &Value,
    updated_by: Uuid,
    mutation_kind: MetadataMutationKind,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_space(&mut tx, space_id).await?;
    let current = sqlx::query_as::<_, NodeRow>(&format!(
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
    let node_kind = current.kind.clone();
    if current.metadata == *metadata {
        tx.commit().await.map_err(map_sqlx_error)?;
        return current.into_node();
    }

    let row = sqlx::query_as::<_, NodeRow>(&format!(
        "UPDATE nodes \
         SET metadata = $3, updated_by_account_id = $4, updated_at = now() \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING {NODE_COLUMNS}"
    ))
    .bind(space_id)
    .bind(node_id)
    .bind(metadata)
    .bind(updated_by)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_constraint_error)?
    .ok_or_else(|| Error::not_found("node not found"))?;

    file_change_events::node_metadata_replaced(
        &mut tx,
        file_change_events::context(updated_by, space_id),
        node_id,
        mutation_kind,
        &node_kind,
        &row.name,
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}
