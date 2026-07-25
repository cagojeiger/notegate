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
use notegate_model::files::UpdateNode;
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
    command: &UpdateNode,
    updated_by: Uuid,
    caps: notegate_core::limits::Limits,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    let locked = checks::lock_space_context(&mut tx, space_id, caps).await?;

    let current = sqlx::query_as::<_, NodeRow>(&format!(
        "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
         FOR UPDATE"
    ))
    .bind(space_id)
    .bind(command.node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::not_found("node not found"))?;
    let node_kind = current.kind.clone();
    let parent_id = current.parent_id;
    let current_name = current.name.clone();
    let current_sort_order = current.sort_order;

    if command.name.is_some() && parent_id.is_none() {
        return Err(Error::conflict("cannot rename the root node"));
    }
    if command.search_enabled.is_some() && parent_id.is_none() {
        return Err(Error::conflict(
            "search policy cannot be changed on the root node",
        ));
    }
    if command.text_encryption_enabled.is_some() && node_kind != "text" {
        return Err(Error::validation(
            "text_encryption_enabled applies only to text nodes",
        ));
    }

    let name_changed = command
        .name
        .as_deref()
        .is_some_and(|name| name != current_name);
    let sort_order_changed = command
        .sort_order
        .is_some_and(|sort_order| sort_order != current_sort_order);
    let search_enabled_changed = command
        .search_enabled
        .is_some_and(|enabled| enabled != current.search_enabled);
    let current_text_policy: Option<(String, bool)> = if node_kind == "text" {
        sqlx::query_as(
            "SELECT storage_format, encryption_enabled FROM text_objects \
             WHERE space_id = $1 AND node_id = $2 FOR UPDATE",
        )
        .bind(space_id)
        .bind(command.node_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
    } else {
        None
    };
    if node_kind == "text" && current_text_policy.is_none() {
        return Err(Error::internal("text node has no text object"));
    }
    let text_encryption_changed = command.text_encryption_enabled.is_some_and(|enabled| {
        current_text_policy
            .as_ref()
            .is_some_and(|(_, current)| enabled != *current)
    });
    if text_encryption_changed && command.text_encryption_enabled == Some(true) {
        if !locked.owner_tier.features().text_encryption {
            return Err(Error::conflict(
                "text encryption is not available for the space owner's tier",
            ));
        }
        if current_text_policy
            .as_ref()
            .is_some_and(|(format, _)| format != "plain")
        {
            return Err(Error::conflict(
                "server text encryption requires plain text storage",
            ));
        }
    }
    if !name_changed && !sort_order_changed && !search_enabled_changed && !text_encryption_changed {
        tx.commit().await.map_err(map_sqlx_error)?;
        return current.into_node();
    }

    if let Some(name) = command.name.as_deref()
        && name_changed
    {
        let Some(parent_id) = parent_id else {
            return Err(Error::conflict("cannot rename the root node"));
        };
        checks::require_sibling_unique(&mut tx, space_id, parent_id, name, Some(command.node_id))
            .await?;
    }

    let row = sqlx::query_as::<_, NodeRow>(&format!(
        "UPDATE nodes \
         SET name = COALESCE($3, name), \
             sort_order = COALESCE($4, sort_order), \
             search_enabled = COALESCE($5, search_enabled), \
             updated_by_account_id = $6, updated_at = now() \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING {NODE_COLUMNS}"
    ))
    .bind(space_id)
    .bind(command.node_id)
    .bind(command.name.as_deref())
    .bind(command.sort_order)
    .bind(command.search_enabled)
    .bind(updated_by)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_constraint_error)?
    .ok_or_else(|| Error::not_found("node not found"))?;

    if text_encryption_changed {
        sqlx::query(
            "UPDATE text_objects \
             SET encryption_enabled = $3, updated_by_account_id = $4, updated_at = now() \
             WHERE space_id = $1 AND node_id = $2",
        )
        .bind(space_id)
        .bind(command.node_id)
        .bind(command.text_encryption_enabled)
        .bind(updated_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
    }

    file_change_events::node_updated(
        &mut tx,
        file_change_events::context(updated_by, space_id),
        command.node_id,
        file_change_events::NodeUpdated {
            item_kind: &node_kind,
            item_name: &row.name,
            parent_node_id: row.parent_id,
            name_changed,
            sort_order_changed,
            search_enabled_changed,
            text_encryption_changed,
            search_enabled: row.search_enabled,
            text_encryption_enabled: if node_kind == "text" {
                command
                    .text_encryption_enabled
                    .or_else(|| current_text_policy.as_ref().map(|(_, enabled)| *enabled))
            } else {
                None
            },
        },
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
        row.parent_id,
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}
