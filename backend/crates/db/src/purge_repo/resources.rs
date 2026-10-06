//! Reclaim deleted resources and queue their object deletion before removing ownership.

use crate::map_sqlx_error;
use notegate_core::Result;
use sqlx::{PgPool, Row as _};
use uuid::Uuid;

const SPACE_PURGE_BATCH: i64 = 100;
const NODE_PURGE_BATCH: i64 = 1_000;
const LINK_GRAPH_PROJECTION_PURGE_BATCH: i64 = 1_000;

pub(super) struct PurgedResources {
    pub(super) spaces_deleted: i64,
    pub(super) nodes_deleted: i64,
    pub(super) link_graph_projections_deleted: i64,
    pub(super) object_deletions_queued: u64,
}

pub(super) async fn purge(pool: &PgPool) -> Result<PurgedResources> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    // Lock each affected Space before selecting due IDs. Restore and delete
    // share this serialization boundary; queue and cascade use one snapshot.
    let candidates: Vec<Uuid> = sqlx::query_scalar(
        "WITH due_spaces AS (SELECT id AS space_id FROM spaces \
             WHERE deleted_at IS NOT NULL AND (purge_requested_at IS NOT NULL OR purge_after <= now()) \
             ORDER BY purge_after, id LIMIT $1), \
         due_nodes AS (SELECT space_id FROM nodes \
             WHERE deleted_at IS NOT NULL AND (purge_requested_at IS NOT NULL OR purge_after <= now()) \
             ORDER BY purge_after, id LIMIT $2) \
         SELECT space_id FROM due_spaces UNION SELECT space_id FROM due_nodes ORDER BY space_id",
    )
    .bind(SPACE_PURGE_BATCH)
    .bind(NODE_PURGE_BATCH)
    .fetch_all(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let mut locked_spaces = Vec::new();
    for space_id in candidates {
        if !crate::space_usage::try_acquire_reconciliation_gate(&mut tx, space_id).await? {
            continue;
        }
        let locked: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM spaces WHERE id = $1 FOR UPDATE SKIP LOCKED")
                .bind(space_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        if let Some(id) = locked {
            locked_spaces.push(id);
        }
    }
    let due_spaces: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM spaces WHERE id = ANY($1) AND deleted_at IS NOT NULL AND (purge_requested_at IS NOT NULL OR purge_after <= now()) \
         ORDER BY purge_after, id LIMIT $2",
    ).bind(&locked_spaces).bind(SPACE_PURGE_BATCH)
        .fetch_all(&mut *tx).await.map_err(map_sqlx_error)?;
    let due_nodes: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM nodes WHERE space_id = ANY($1) AND deleted_at IS NOT NULL AND (purge_requested_at IS NOT NULL OR purge_after <= now()) \
         ORDER BY purge_after, id LIMIT $2",
    ).bind(&locked_spaces).bind(NODE_PURGE_BATCH)
        .fetch_all(&mut *tx).await.map_err(map_sqlx_error)?;
    let object_deletions_queued = sqlx::query(
        "WITH RECURSIVE due_nodes AS ( \
             SELECT id FROM nodes WHERE id = ANY($2) \
             UNION SELECT child.id FROM nodes child JOIN due_nodes parent ON child.parent_id = parent.id \
         ) UPDATE object_storage_objects f SET state = 'delete_pending', \
             delete_requested_at = COALESCE(delete_requested_at, now()), \
             retry_after = NULL, last_error_code = NULL \
         WHERE f.state = 'attached' AND (f.space_id = ANY($1) OR f.node_id IN (SELECT id FROM due_nodes))",
    ).bind(&due_spaces).bind(&due_nodes).execute(&mut *tx).await.map_err(map_sqlx_error)?.rows_affected();
    let spaces_deleted = sqlx::query("DELETE FROM spaces WHERE id = ANY($1)")
        .bind(&due_spaces)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected() as i64;
    let nodes_deleted = sqlx::query("DELETE FROM nodes WHERE id = ANY($1)")
        .bind(&due_nodes)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected() as i64;

    // This work ledger intentionally has no Space/node FK so enqueueing it
    // cannot invert mutation lock ordering. Reclaim only rows whose owners
    // have already been hard-deleted.
    let link_graph_projections_deleted: i64 = sqlx::query(
        "WITH due AS ( \
             SELECT projection.space_id, projection.source_node_id \
             FROM node_link_projections projection \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM spaces space WHERE space.id = projection.space_id \
             ) OR NOT EXISTS ( \
                 SELECT 1 FROM nodes node \
                 WHERE node.space_id = projection.space_id \
                   AND node.id = projection.source_node_id \
             ) \
             ORDER BY projection.space_id, projection.source_node_id \
             LIMIT $1 FOR UPDATE OF projection SKIP LOCKED \
         ), deleted AS ( \
             DELETE FROM node_link_projections projection USING due \
             WHERE projection.space_id = due.space_id \
               AND projection.source_node_id = due.source_node_id \
             RETURNING projection.source_node_id \
         ) \
         SELECT count(*) AS deleted_count FROM deleted",
    )
    .bind(LINK_GRAPH_PROJECTION_PURGE_BATCH)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .get("deleted_count");

    tx.commit().await.map_err(map_sqlx_error)?;
    tracing::info!(
        event = "purge.group_completed",
        group = "resources",
        spaces_deleted,
        nodes_deleted,
        link_graph_projections_deleted,
        object_deletions_queued,
    );

    Ok(PurgedResources {
        spaces_deleted,
        nodes_deleted,
        link_graph_projections_deleted,
        object_deletions_queued,
    })
}
