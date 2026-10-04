//! Reclaim deleted resources and queue their object deletion before removing ownership.

use crate::map_sqlx_error;
use notegate_core::Result;
use sqlx::{PgPool, Row as _};

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

    // Safety net for requests missed during soft delete: queue physical
    // object deletion before semantic rows disappear. The operational
    // ledger survives the following cascades and is processed outside this
    // transaction by object-storage cleanup reconciliation.
    let queued_for_spaces = sqlx::query(
        "UPDATE object_storage_objects f SET \
             state = 'delete_pending', \
             delete_requested_at = COALESCE(delete_requested_at, now()), \
             retry_after = NULL, last_error_code = NULL \
         WHERE f.state = 'attached' AND f.space_id IN ( \
             SELECT id FROM spaces \
             WHERE deleted_at IS NOT NULL AND purge_after <= now() \
             ORDER BY purge_after, id LIMIT $1 \
         )",
    )
    .bind(SPACE_PURGE_BATCH)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();

    let queued_for_nodes = sqlx::query(
        "WITH RECURSIVE due_roots AS ( \
             SELECT id FROM nodes \
             WHERE deleted_at IS NOT NULL AND purge_after <= now() \
             ORDER BY purge_after, id LIMIT $1 \
         ), due_nodes AS ( \
             SELECT id FROM due_roots \
             UNION \
             SELECT child.id FROM nodes child \
             JOIN due_nodes parent ON child.parent_id = parent.id \
         ) \
         UPDATE object_storage_objects f SET \
             state = 'delete_pending', \
             delete_requested_at = COALESCE(delete_requested_at, now()), \
             retry_after = NULL, last_error_code = NULL \
         WHERE f.state = 'attached' AND f.node_id IN (SELECT id FROM due_nodes)",
    )
    .bind(NODE_PURGE_BATCH)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();

    // Space hard delete cascades agent connections, nodes, text objects, and file objects.
    let spaces_deleted: i64 = sqlx::query(
        "WITH due AS ( \
             SELECT id FROM spaces \
             WHERE deleted_at IS NOT NULL AND purge_after <= now() \
             ORDER BY purge_after, id \
             LIMIT $1 \
         ), deleted AS ( \
             DELETE FROM spaces w USING due \
             WHERE w.id = due.id \
             RETURNING w.id \
         ) \
         SELECT count(*) AS deleted_count FROM deleted",
    )
    .bind(SPACE_PURGE_BATCH)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .get("deleted_count");

    // Node hard delete cascades text/file objects and any descendant nodes. The CTE
    // limits the number of selected due nodes; cascaded descendants may make
    // the physical row count larger, which is acceptable and bounded by the
    // product subtree/space limits.
    let nodes_deleted: i64 = sqlx::query(
        "WITH due AS ( \
             SELECT id FROM nodes \
             WHERE deleted_at IS NOT NULL AND purge_after <= now() \
             ORDER BY purge_after, id \
             LIMIT $1 \
         ), deleted AS ( \
             DELETE FROM nodes n USING due \
             WHERE n.id = due.id \
             RETURNING n.id \
         ) \
         SELECT count(*) AS deleted_count FROM deleted",
    )
    .bind(NODE_PURGE_BATCH)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .get("deleted_count");

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

    let object_deletions_queued = queued_for_spaces + queued_for_nodes;
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
