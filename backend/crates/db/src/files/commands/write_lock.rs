//! Direct node write-lock updates.

use notegate_core::limits::Limits;
use notegate_core::{Error, Result};
use notegate_model::Node;
use notegate_model::files::UpdateNodeWriteLock;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::map_sqlx_error;
use super::super::rows::{NODE_COLUMNS, NodeRow};
use super::{checks, lock_live_node};
use crate::file_change_events;

pub async fn update_node_write_lock(
    pool: &PgPool,
    space_id: Uuid,
    command: &UpdateNodeWriteLock,
    updated_by: Uuid,
    caps: Limits,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    let locked = checks::lock_space_context(&mut tx, space_id, caps).await?;
    let current = lock_live_node(&mut tx, space_id, command.node_id).await?;
    checks::require_revision(current.revision, command.expected_revision)?;
    if current.parent_id.is_none() {
        return Err(Error::conflict("cannot change the root node write lock"));
    }
    if command.enabled == current.write_locked {
        tx.commit().await.map_err(map_sqlx_error)?;
        return current.into_node();
    }
    if command.enabled && !locked.owner_tier.features().write_lock {
        return Err(Error::conflict(
            "write lock is not available for the space owner's tier",
        ));
    }

    let row = sqlx::query_as::<_, NodeRow>(sqlx::AssertSqlSafe(format!(
        "UPDATE nodes \
         SET write_locked = $3, updated_by_account_id = $4, updated_at = now() \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING {NODE_COLUMNS}"
    )))
    .bind(space_id)
    .bind(command.node_id)
    .bind(command.enabled)
    .bind(updated_by)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::not_found("node not found"))?;

    file_change_events::node_write_lock_updated(
        &mut tx,
        file_change_events::context(updated_by, space_id),
        command.node_id,
        &row.kind,
        &row.name,
        row.parent_id,
        row.write_locked,
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}
