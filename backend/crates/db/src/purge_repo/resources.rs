//! Incremental, irreversible resource cleanup. Each Space batch commits separately.

use std::time::{Duration, Instant};

use crate::map_sqlx_error;
use notegate_core::{Error, Result};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

const SPACE_PURGE_BATCH: i64 = 10;
const NODE_PURGE_BATCH: i64 = 100;
const REVISION_PURGE_BATCH: i64 = 100;
const OBJECT_PURGE_BATCH: i64 = 100;
const LINK_REF_PURGE_BATCH: i64 = 1_000;
const CONNECTION_PURGE_BATCH: i64 = 100;
const LINK_GRAPH_PROJECTION_PURGE_BATCH: i64 = 1_000;
const RESOURCE_PASS_BUDGET: Duration = Duration::from_secs(30);
const SPACE_BATCH_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Default)]
pub(super) struct PurgedResources {
    pub(super) spaces_deleted: u64,
    pub(super) nodes_deleted: u64,
    pub(super) text_revisions_deleted: u64,
    pub(super) link_graph_projections_deleted: u64,
    pub(super) object_deletions_queued: u64,
    pub(super) has_more: bool,
}

pub(super) async fn purge(pool: &PgPool) -> Result<PurgedResources> {
    let started = Instant::now();
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
    set_timeouts(&mut tx).await?;
    // Persist rotation before doing work, including skipped/failed Spaces. No
    // in-memory cursor is needed after restart and one large Space cannot fill
    // the candidate window. This does not change deletion eligibility.
    let candidates: Vec<Uuid> = sqlx::query_scalar(
        "WITH candidates AS ( \
             SELECT s.id FROM spaces s WHERE \
                 (s.deleted_at IS NOT NULL AND (s.purge_requested_at IS NOT NULL OR s.purge_after <= now())) \
                 OR EXISTS (SELECT 1 FROM nodes n WHERE n.space_id = s.id AND n.deleted_at IS NOT NULL \
                     AND (n.purge_requested_at IS NOT NULL OR n.purge_after <= now())) \
             ORDER BY s.purge_last_attempt_at NULLS FIRST, s.id \
             LIMIT $1 FOR UPDATE OF s SKIP LOCKED \
         ) UPDATE spaces s SET purge_last_attempt_at = clock_timestamp() \
         FROM candidates c WHERE s.id = c.id RETURNING s.id",
    )
    .bind(SPACE_PURGE_BATCH)
    .fetch_all(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    tx.commit().await.map_err(map_sqlx_error)?;

    let mut result = PurgedResources::default();
    let mut first_error = None;
    for space_id in candidates {
        if started.elapsed() >= RESOURCE_PASS_BUDGET {
            break;
        }
        let batch = tokio::time::timeout(SPACE_BATCH_TIMEOUT, purge_space(pool, space_id))
            .await
            .unwrap_or_else(|_| Err(Error::internal("Space purge batch timed out")));
        match batch {
            Ok(batch) => {
                result.spaces_deleted += batch.spaces_deleted;
                result.nodes_deleted += batch.nodes_deleted;
                result.text_revisions_deleted += batch.text_revisions_deleted;
                result.link_graph_projections_deleted += batch.link_graph_projections_deleted;
                result.object_deletions_queued += batch.object_deletions_queued;
            }
            Err(error) => {
                tracing::warn!(event = "purge.space_failed", %space_id, %error);
                first_error.get_or_insert(error);
            }
        }
    }
    // This ledger has no owner FK. Keep orphan cleanup bounded and independent
    // of committed Space batches (including owners removed by older binaries).
    let orphan_cleanup = purge_orphan_projections(pool).await;
    match orphan_cleanup {
        Ok(deleted) => result.link_graph_projections_deleted += deleted,
        Err(error) => {
            first_error.get_or_insert(error);
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }

    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
    set_timeouts(&mut tx).await?;
    let (pending_candidates, oldest_due): (i64, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT count(*), min(due_at) FROM ( \
             SELECT id AS space_id, LEAST(purge_after, purge_requested_at) AS due_at \
             FROM spaces WHERE deleted_at IS NOT NULL AND (purge_requested_at IS NOT NULL OR purge_after <= now()) \
             UNION ALL SELECT space_id, LEAST(purge_after, purge_requested_at) FROM nodes \
             WHERE deleted_at IS NOT NULL AND (purge_requested_at IS NOT NULL OR purge_after <= now()) \
         ) due",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    tx.commit().await.map_err(map_sqlx_error)?;
    // Count above is deletion candidates, not physical rows or S3 completions.
    result.has_more = pending_candidates > 0
        || result.link_graph_projections_deleted >= LINK_GRAPH_PROJECTION_PURGE_BATCH as u64;
    tracing::info!(
        event = "purge.group_completed",
        group = "resources",
        spaces_deleted = result.spaces_deleted,
        nodes_deleted = result.nodes_deleted,
        text_revisions_deleted = result.text_revisions_deleted,
        link_graph_projections_deleted = result.link_graph_projections_deleted,
        object_deletions_queued = result.object_deletions_queued,
        pending_candidates,
        ?oldest_due,
        elapsed_ms = started.elapsed().as_millis() as u64,
    );
    Ok(result)
}

async fn set_timeouts(connection: &mut PgConnection) -> Result<()> {
    sqlx::query("SET LOCAL lock_timeout = '2s'")
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    sqlx::query("SET LOCAL statement_timeout = '10s'")
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    Ok(())
}

async fn purge_space(pool: &PgPool, space_id: Uuid) -> Result<PurgedResources> {
    let started = Instant::now();
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
    set_timeouts(&mut tx).await?;
    if !crate::space_usage::try_acquire_reconciliation_gate(&mut tx, space_id).await? {
        return Ok(PurgedResources::default());
    }
    let due_space: Option<bool> = sqlx::query_scalar(
        "SELECT deleted_at IS NOT NULL AND (purge_requested_at IS NOT NULL OR purge_after <= now()) \
         FROM spaces WHERE id = $1 FOR UPDATE SKIP LOCKED",
    ).bind(space_id).fetch_optional(&mut *tx).await.map_err(map_sqlx_error)?;
    let Some(due_space) = due_space else {
        return Ok(PurgedResources::default());
    };
    // Walk ancestors of leaves rather than expanding/cascading the entire tree.
    // A due ancestor owns its physical subtree, including separately trashed
    // children whose own retention deadline is later.
    let leaves: Vec<Uuid> = sqlx::query_scalar(
        "SELECT n.id FROM nodes n WHERE n.space_id = $1 \
         AND NOT EXISTS (SELECT 1 FROM nodes child WHERE child.space_id = $1 AND child.parent_id = n.id) \
         AND ($2 OR EXISTS ( \
             WITH RECURSIVE ancestors AS ( \
                 SELECT id, parent_id, deleted_at, purge_after, purge_requested_at FROM nodes WHERE id = n.id \
                 UNION ALL SELECT p.id, p.parent_id, p.deleted_at, p.purge_after, p.purge_requested_at \
                 FROM nodes p JOIN ancestors a ON p.id = a.parent_id WHERE p.space_id = $1 \
             ) SELECT 1 FROM ancestors WHERE deleted_at IS NOT NULL \
                 AND (purge_requested_at IS NOT NULL OR purge_after <= now()) \
         )) ORDER BY n.id LIMIT $3 FOR UPDATE OF n SKIP LOCKED",
    ).bind(space_id).bind(due_space).bind(NODE_PURGE_BATCH)
        .fetch_all(&mut *tx).await.map_err(map_sqlx_error)?;
    let text_revisions_deleted = sqlx::query(
        "WITH due AS (SELECT id FROM text_revisions WHERE space_id = $1 AND node_id = ANY($2) \
             ORDER BY node_id, id LIMIT $3 FOR UPDATE SKIP LOCKED) \
         DELETE FROM text_revisions r USING due WHERE r.id = due.id",
    )
    .bind(space_id)
    .bind(&leaves)
    .bind(REVISION_PURGE_BATCH)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();
    let mut result = PurgedResources {
        text_revisions_deleted,
        ..PurgedResources::default()
    };

    // Drain both sides of link FKs before deleting the node. Incoming links
    // retain their paths, just as ON DELETE SET NULL did, but in bounded work.
    sqlx::query(
        "WITH due AS (SELECT space_id, source_node_id, reference_kind, target_path FROM node_link_refs \
             WHERE space_id = $1 AND source_node_id = ANY($2) \
             ORDER BY source_node_id, reference_kind, target_path LIMIT $3 FOR UPDATE SKIP LOCKED) \
         DELETE FROM node_link_refs r USING due WHERE r.space_id = due.space_id \
             AND r.source_node_id = due.source_node_id AND r.reference_kind = due.reference_kind AND r.target_path = due.target_path",
    ).bind(space_id).bind(&leaves).bind(LINK_REF_PURGE_BATCH)
        .execute(&mut *tx).await.map_err(map_sqlx_error)?;
    sqlx::query(
        "WITH due AS (SELECT space_id, source_node_id, reference_kind, target_path FROM node_link_refs \
             WHERE space_id = $1 AND target_node_id = ANY($2) \
             ORDER BY target_node_id, source_node_id, reference_kind, target_path LIMIT $3 FOR UPDATE SKIP LOCKED) \
         UPDATE node_link_refs r SET target_node_id = NULL FROM due WHERE r.space_id = due.space_id \
             AND r.source_node_id = due.source_node_id AND r.reference_kind = due.reference_kind AND r.target_path = due.target_path",
    ).bind(space_id).bind(&leaves).bind(LINK_REF_PURGE_BATCH)
        .execute(&mut *tx).await.map_err(map_sqlx_error)?;

    // Attached object intent and semantic deletion commit together. Pending
    // uploads under an irreversible folder are expired; do not reset existing
    // cleanup leases/backoff when detaching ledger references.
    let queued: i64 = sqlx::query_scalar(
        "WITH due AS (SELECT f.id, f.state, COALESCE(f.deletion_operation_id, n.deletion_operation_id, p.deletion_operation_id, s.deletion_operation_id) AS operation_id \
             FROM object_storage_objects f JOIN spaces s ON s.id = $1 \
             LEFT JOIN nodes n ON n.id = f.node_id LEFT JOIN nodes p ON p.id = f.parent_node_id \
             WHERE f.parent_node_id = ANY($2) OR (f.node_id = ANY($2) \
                 AND NOT EXISTS (SELECT 1 FROM text_revisions r WHERE r.space_id = $1 AND r.node_id = f.node_id) \
                 AND NOT EXISTS (SELECT 1 FROM node_link_refs r WHERE r.space_id = $1 AND (r.source_node_id = f.node_id OR r.target_node_id = f.node_id)) \
                 AND NOT EXISTS (SELECT 1 FROM object_storage_objects anchor WHERE anchor.parent_node_id = f.node_id)) \
             ORDER BY f.id LIMIT $3 FOR UPDATE OF f SKIP LOCKED), changed AS ( \
             UPDATE object_storage_objects f SET \
                 state = CASE f.state WHEN 'attached' THEN 'delete_pending' WHEN 'uploading' THEN 'expire_pending' ELSE f.state END, \
                 delete_requested_at = CASE WHEN f.state IN ('attached', 'uploading') THEN COALESCE(f.delete_requested_at, now()) ELSE f.delete_requested_at END, \
                 deletion_operation_id = due.operation_id, \
                 parent_node_id = CASE WHEN f.parent_node_id = ANY($2) THEN NULL ELSE f.parent_node_id END, \
                 retry_after = CASE WHEN f.state IN ('attached', 'uploading') THEN NULL ELSE f.retry_after END, \
                 last_error_code = CASE WHEN f.state IN ('attached', 'uploading') THEN NULL ELSE f.last_error_code END \
             FROM due \
             WHERE f.id = due.id RETURNING due.state \
         ) SELECT count(*) FROM changed WHERE state IN ('attached', 'uploading')",
    ).bind(space_id).bind(&leaves).bind(OBJECT_PURGE_BATCH)
        .fetch_one(&mut *tx).await.map_err(map_sqlx_error)?;
    result.object_deletions_queued += queued as u64;

    let ready: Vec<Uuid> = sqlx::query_scalar(
        "SELECT n.id FROM nodes n WHERE n.space_id = $1 AND n.id = ANY($2) AND n.parent_id IS NOT NULL \
         AND NOT EXISTS (SELECT 1 FROM text_revisions r WHERE r.space_id = $1 AND r.node_id = n.id) \
         AND NOT EXISTS (SELECT 1 FROM node_link_refs r WHERE r.space_id = $1 AND (r.source_node_id = n.id OR r.target_node_id = n.id)) \
         AND NOT EXISTS (SELECT 1 FROM object_storage_objects f WHERE f.parent_node_id = n.id OR (f.node_id = n.id AND f.state = 'attached'))",
    ).bind(space_id).bind(&leaves).fetch_all(&mut *tx).await.map_err(map_sqlx_error)?;
    result.nodes_deleted = sqlx::query("DELETE FROM nodes WHERE space_id = $1 AND id = ANY($2)")
        .bind(space_id)
        .bind(&ready)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected();
    result.link_graph_projections_deleted = sqlx::query(
        "DELETE FROM node_link_projections WHERE space_id = $1 AND source_node_id = ANY($2)",
    )
    .bind(space_id)
    .bind(&ready)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();

    // Finalize a Space only after all non-root nodes have gone. Drain its
    // remaining ledger anchors/connections too, avoiding another large cascade.
    let empty: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM nodes WHERE space_id = $1 AND parent_id IS NOT NULL)",
    )
    .bind(space_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if due_space && empty {
        result.object_deletions_queued += sqlx::query_scalar::<_, i64>(
            "WITH due AS (SELECT id, state FROM object_storage_objects WHERE space_id = $1 \
                 ORDER BY id LIMIT $2 FOR UPDATE SKIP LOCKED), changed AS ( \
                 UPDATE object_storage_objects f SET space_id = NULL, parent_node_id = NULL, \
                     state = CASE f.state WHEN 'uploading' THEN 'expire_pending' ELSE f.state END, \
                     deletion_operation_id = COALESCE(f.deletion_operation_id, s.deletion_operation_id), \
                     delete_requested_at = CASE WHEN f.state = 'uploading' THEN COALESCE(f.delete_requested_at, now()) ELSE f.delete_requested_at END, \
                     retry_after = CASE WHEN f.state = 'uploading' THEN NULL ELSE f.retry_after END, \
                     last_error_code = CASE WHEN f.state = 'uploading' THEN NULL ELSE f.last_error_code END \
                 FROM due, spaces s WHERE f.id = due.id AND s.id = $1 RETURNING due.state \
             ) SELECT count(*) FROM changed WHERE state = 'uploading'",
        ).bind(space_id).bind(OBJECT_PURGE_BATCH).fetch_one(&mut *tx).await.map_err(map_sqlx_error)? as u64;
        sqlx::query(
            "WITH due AS (SELECT agent_id FROM space_agent_connections WHERE space_id = $1 \
                 ORDER BY agent_id LIMIT $2 FOR UPDATE SKIP LOCKED) \
             DELETE FROM space_agent_connections c USING due WHERE c.space_id = $1 AND c.agent_id = due.agent_id",
        ).bind(space_id).bind(CONNECTION_PURGE_BATCH).execute(&mut *tx).await.map_err(map_sqlx_error)?;
        result.spaces_deleted = sqlx::query(
            "DELETE FROM spaces s WHERE s.id = $1 \
             AND NOT EXISTS (SELECT 1 FROM object_storage_objects WHERE space_id = $1) \
             AND NOT EXISTS (SELECT 1 FROM space_agent_connections WHERE space_id = $1) \
             AND NOT EXISTS (SELECT 1 FROM node_link_refs WHERE space_id = $1)",
        )
        .bind(space_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected();
    }
    tx.commit().await.map_err(map_sqlx_error)?;
    tracing::info!(event = "purge.space_completed", %space_id,
        nodes_deleted = result.nodes_deleted, text_revisions_deleted = result.text_revisions_deleted,
        spaces_deleted = result.spaces_deleted, object_deletions_queued = result.object_deletions_queued,
        elapsed_ms = started.elapsed().as_millis() as u64);
    Ok(result)
}

async fn purge_orphan_projections(pool: &PgPool) -> Result<u64> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
    set_timeouts(&mut tx).await?;
    let deleted = sqlx::query(
        "WITH due AS ( \
             SELECT projection.space_id, projection.source_node_id FROM node_link_projections projection \
             WHERE NOT EXISTS (SELECT 1 FROM nodes n WHERE n.space_id = projection.space_id AND n.id = projection.source_node_id) \
             ORDER BY projection.space_id, projection.source_node_id \
             LIMIT $1 FOR UPDATE OF projection SKIP LOCKED \
         ) DELETE FROM node_link_projections projection USING due \
         WHERE projection.space_id = due.space_id AND projection.source_node_id = due.source_node_id",
    ).bind(LINK_GRAPH_PROJECTION_PURGE_BATCH).execute(&mut *tx).await.map_err(map_sqlx_error)?.rows_affected();
    tx.commit().await.map_err(map_sqlx_error)?;
    Ok(deleted)
}
