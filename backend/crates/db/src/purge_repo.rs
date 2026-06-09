//! Hard purge for soft-deleted workspaces and nodes.
//!
//! The purge run is protected by a Postgres advisory transaction lock. Multiple
//! application processes may start this worker, but only one process can execute
//! a purge transaction at a time for a given database.

use crate::map_sqlx_error;
use notegate_core::Result;
use sqlx::{PgPool, Row as _};

/// Stable advisory lock key for notegate purge runs.
///
/// This is an arbitrary signed 64-bit namespace value. It must stay stable so
/// all notegate instances contend on the same database lock.
const PURGE_ADVISORY_LOCK_KEY: i64 = 0x4e47_5055_5247_4501;
const WORKSPACE_PURGE_BATCH: i64 = 100;
const NODE_PURGE_BATCH: i64 = 1_000;

#[derive(Debug, Clone)]
pub struct PurgeRepo {
    pool: PgPool,
}

impl PurgeRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Run one bounded purge attempt.
    ///
    /// Returns immediately with `lock_acquired=false` if another notegate
    /// process is already purging this database.
    pub async fn run_once(&self) -> Result<PurgeRun> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let lock_acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
            .bind(PURGE_ADVISORY_LOCK_KEY)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

        if !lock_acquired {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(PurgeRun {
                lock_acquired: false,
                workspaces_deleted: 0,
                nodes_deleted: 0,
            });
        }

        // Workspace hard delete cascades workspace_access, nodes, and documents.
        let workspaces_deleted: i64 = sqlx::query(
            "WITH due AS ( \
                 SELECT id FROM workspaces \
                 WHERE deleted_at IS NOT NULL AND purge_after <= now() \
                 ORDER BY purge_after, id \
                 LIMIT $1 \
             ), deleted AS ( \
                 DELETE FROM workspaces w USING due \
                 WHERE w.id = due.id \
                 RETURNING w.id \
             ) \
             SELECT count(*) AS deleted_count FROM deleted",
        )
        .bind(WORKSPACE_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("deleted_count");

        // Node hard delete cascades documents and any descendant nodes. The CTE
        // limits the number of selected due nodes; cascaded descendants may make
        // the physical row count larger, which is acceptable and bounded by the
        // product subtree/workspace limits.
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

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(PurgeRun {
            lock_acquired: true,
            workspaces_deleted: workspaces_deleted.max(0) as u64,
            nodes_deleted: nodes_deleted.max(0) as u64,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeRun {
    pub lock_acquired: bool,
    pub workspaces_deleted: u64,
    pub nodes_deleted: u64,
}
