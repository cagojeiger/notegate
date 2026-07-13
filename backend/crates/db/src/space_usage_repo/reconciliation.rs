use chrono::{DateTime, Utc};
use notegate_core::Result;
use serde_json::{Value, json};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{map_sqlx_error, space_usage};

use super::{
    SpaceUsageRepo, UsageCounts, configure_transaction, exact_usage, try_acquire_worker_lock,
};

const RETRY_INTERVAL: &str = "5 minutes";
const STATEMENT_TIMEOUT: &str = "30s";

impl SpaceUsageRepo {
    /// Execute at most one queued Space reconciliation.
    pub async fn execute_next_reconciliation(&self) -> Result<UsageReconcileExecution> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        configure_transaction(&mut tx, STATEMENT_TIMEOUT).await?;

        if !try_acquire_worker_lock(&mut tx).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(UsageReconcileExecution::WorkerLockHeld);
        }

        let Some(job) = next_job(&mut tx).await? else {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(UsageReconcileExecution::Idle);
        };

        sqlx::query("SAVEPOINT usage_reconcile_job")
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        let execution = match execute_job(&mut tx, &job).await {
            Ok(execution) => execution,
            Err(error) => {
                let error_message = error.to_string();
                sqlx::query("ROLLBACK TO SAVEPOINT usage_reconcile_job")
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_error)?;
                let run_after = record_failed_execution(&mut tx, &job, &error_message).await?;
                UsageReconcileExecution::Failed {
                    job_id: job.job_id,
                    space_id: job.space_id,
                    run_after,
                    error: error_message,
                }
            }
        };
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(execution)
    }

    /// Remove execution history past the retention window when this process
    /// acquires the reconciliation worker lock. Returns `false` when another
    /// worker is active.
    pub async fn try_delete_expired_executions(&self) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        configure_transaction(&mut tx, STATEMENT_TIMEOUT).await?;
        if !try_acquire_worker_lock(&mut tx).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(false);
        }

        sqlx::query(
            "DELETE FROM space_usage_reconcile_executions \
             WHERE finished_at < now() - interval '3 months'",
        )
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(true)
    }
}

async fn execute_job(
    tx: &mut sqlx::PgConnection,
    job: &QueuedJob,
) -> Result<UsageReconcileExecution> {
    if !space_usage::try_acquire_reconciliation_gate(tx, job.space_id).await? {
        let run_after: DateTime<Utc> = sqlx::query_scalar(
            "UPDATE space_usage_reconcile_jobs \
             SET run_after = now() + $2::interval \
             WHERE job_id = $1 RETURNING run_after",
        )
        .bind(job.job_id)
        .bind(RETRY_INTERVAL)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        insert_execution(tx, job, "deferred", None, json!({"reason": "space_busy"})).await?;
        return Ok(UsageReconcileExecution::Deferred {
            job_id: job.job_id,
            space_id: job.space_id,
            run_after,
        });
    }

    let live_space: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM spaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE")
            .bind(job.space_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
    if live_space.is_none() {
        sqlx::query("DELETE FROM space_usage_reconcile_jobs WHERE job_id = $1")
            .bind(job.job_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        insert_execution(
            tx,
            job,
            "cancelled",
            None,
            json!({"reason": "space_deleted"}),
        )
        .await?;
        return Ok(UsageReconcileExecution::Cancelled {
            job_id: job.job_id,
            space_id: job.space_id,
        });
    }

    // A missing counter row is repaired here: reconciliation is the recovery
    // path for a Space whose counter was lost, so upsert instead of update.
    let previous = sqlx::query_as::<_, UsageCounts>(
        "SELECT live_node_count, live_text_bytes, live_file_bytes \
         FROM space_usage WHERE space_id = $1 FOR UPDATE",
    )
    .bind(job.space_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let actual = exact_usage(tx, job.space_id).await?;

    sqlx::query(
        "INSERT INTO space_usage ( \
             space_id, live_node_count, live_text_bytes, live_file_bytes, reconciled_at \
         ) VALUES ($1, $2, $3, $4, now()) \
         ON CONFLICT (space_id) DO UPDATE \
         SET live_node_count = EXCLUDED.live_node_count, \
             live_text_bytes = EXCLUDED.live_text_bytes, \
             live_file_bytes = EXCLUDED.live_file_bytes, \
             reconciled_at = EXCLUDED.reconciled_at",
    )
    .bind(job.space_id)
    .bind(actual.live_node_count)
    .bind(actual.live_text_bytes)
    .bind(actual.live_file_bytes)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    insert_execution(
        tx,
        job,
        "succeeded",
        None,
        json!({
            "previous_nodes": previous.map(|counts| counts.live_node_count),
            "actual_nodes": actual.live_node_count,
            "previous_text_bytes": previous.map(|counts| counts.live_text_bytes),
            "actual_text_bytes": actual.live_text_bytes,
            "previous_file_bytes": previous.map(|counts| counts.live_file_bytes),
            "actual_file_bytes": actual.live_file_bytes,
        }),
    )
    .await?;
    sqlx::query("DELETE FROM space_usage_reconcile_jobs WHERE job_id = $1")
        .bind(job.job_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

    Ok(UsageReconcileExecution::Succeeded {
        job_id: job.job_id,
        space_id: job.space_id,
        previous,
        actual,
    })
}

async fn record_failed_execution(
    tx: &mut sqlx::PgConnection,
    job: &QueuedJob,
    error_message: &str,
) -> Result<DateTime<Utc>> {
    let run_after: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE space_usage_reconcile_jobs \
         SET retry_count = retry_count + 1, run_after = now() + $2::interval \
         WHERE job_id = $1 RETURNING run_after",
    )
    .bind(job.job_id)
    .bind(RETRY_INTERVAL)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    insert_execution(tx, job, "failed", Some(error_message), json!({})).await?;
    Ok(run_after)
}

async fn insert_execution(
    tx: &mut sqlx::PgConnection,
    job: &QueuedJob,
    outcome: &str,
    error_message: Option<&str>,
    metadata: Value,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO space_usage_reconcile_executions ( \
             job_id, space_id, started_at, outcome, error_message, metadata \
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(job.job_id)
    .bind(job.space_id)
    .bind(job.started_at)
    .bind(outcome)
    .bind(error_message)
    .bind(metadata)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

async fn next_job(tx: &mut sqlx::PgConnection) -> Result<Option<QueuedJob>> {
    sqlx::query_as(
        "SELECT job_id, space_id, now() AS started_at \
         FROM space_usage_reconcile_jobs \
         WHERE run_after <= now() \
         ORDER BY run_after, requested_at, job_id \
         LIMIT 1 FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)
}

#[derive(Debug, Clone, FromRow, PartialEq, Eq)]
struct QueuedJob {
    job_id: Uuid,
    space_id: Uuid,
    started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageReconcileExecution {
    WorkerLockHeld,
    Idle,
    Deferred {
        job_id: Uuid,
        space_id: Uuid,
        run_after: DateTime<Utc>,
    },
    Cancelled {
        job_id: Uuid,
        space_id: Uuid,
    },
    Succeeded {
        job_id: Uuid,
        space_id: Uuid,
        /// `None` when the counter row was missing and had to be recreated.
        previous: Option<UsageCounts>,
        actual: UsageCounts,
    },
    Failed {
        job_id: Uuid,
        space_id: Uuid,
        run_after: DateTime<Utc>,
        error: String,
    },
}
