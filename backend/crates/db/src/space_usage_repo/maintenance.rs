use notegate_core::Result;

use crate::{map_sqlx_error, space_usage};

use super::{SpaceUsageRepo, configure_transaction, try_acquire_worker_lock};

const STATEMENT_TIMEOUT: &str = "5min";

impl SpaceUsageRepo {
    /// Rebuild every live Space counter in one maintenance transaction.
    pub async fn execute_full_recalculation(&self) -> Result<FullUsageReconcileExecution> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        configure_transaction(&mut tx, STATEMENT_TIMEOUT).await?;

        if !try_acquire_worker_lock(&mut tx).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(FullUsageReconcileExecution::WorkerLockHeld);
        }
        if !space_usage::try_acquire_full_reconciliation_gate(&mut tx).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(FullUsageReconcileExecution::MutationsActive);
        }

        let result = sqlx::query(
            "INSERT INTO space_usage ( \
                 space_id, live_node_count, live_content_bytes, reconciled_at \
             ) \
             SELECT \
                 s.id, \
                 count(n.id) FILTER (WHERE n.deleted_at IS NULL)::bigint, \
                 COALESCE(sum(t.byte_len) FILTER (WHERE n.deleted_at IS NULL), 0)::bigint + \
                 COALESCE(sum(f.byte_len) FILTER (WHERE n.deleted_at IS NULL), 0)::bigint, \
                 now() \
             FROM spaces s \
             LEFT JOIN nodes n ON n.space_id = s.id \
             LEFT JOIN text_objects t ON t.space_id = n.space_id AND t.node_id = n.id \
             LEFT JOIN file_objects f ON f.space_id = n.space_id AND f.node_id = n.id \
             WHERE s.deleted_at IS NULL \
             GROUP BY s.id \
             ON CONFLICT (space_id) DO UPDATE \
             SET live_node_count = EXCLUDED.live_node_count, \
                 live_content_bytes = EXCLUDED.live_content_bytes, \
                 reconciled_at = EXCLUDED.reconciled_at",
        )
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        sqlx::query(
            "INSERT INTO space_usage_reconcile_executions ( \
                 job_id, space_id, started_at, outcome, metadata \
             ) \
             SELECT job_id, space_id, now(), 'succeeded', \
                    jsonb_build_object('reason', 'full_recalculation') \
             FROM space_usage_reconcile_jobs",
        )
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        sqlx::query("DELETE FROM space_usage_reconcile_jobs")
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(FullUsageReconcileExecution::Recalculated {
            spaces_recalculated: result.rows_affected(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullUsageReconcileExecution {
    WorkerLockHeld,
    MutationsActive,
    Recalculated { spaces_recalculated: u64 },
}
