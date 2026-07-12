//! Exact reconciliation for transactionally maintained Space usage counters.

use chrono::{DateTime, Utc};
use notegate_core::{Error, Result};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{map_sqlx_error, space_usage};

const RECONCILE_ADVISORY_LOCK_SEED: i64 = 0x4e47_5553_4147_4501;
const RETRY_INTERVAL: &str = "5 minutes";
const LOCK_TIMEOUT: &str = "5s";
const RECONCILE_STATEMENT_TIMEOUT: &str = "30s";
const FULL_RECALCULATION_STATEMENT_TIMEOUT: &str = "5min";

#[derive(Debug, Clone)]
pub struct SpaceUsageRepo {
    pool: PgPool,
}

impl SpaceUsageRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Require the usage tables and active Space creation trigger from migration 0012.
    pub async fn require_schema(&self) -> Result<()> {
        let installed: bool = sqlx::query_scalar(
            "SELECT EXISTS ( \
                        SELECT 1 FROM pg_class c \
                        JOIN pg_namespace n ON n.oid = c.relnamespace \
                        WHERE n.nspname = current_schema() \
                          AND c.relname = 'space_usage' AND c.relkind = 'r' \
                    ) \
                    AND EXISTS ( \
                        SELECT 1 FROM pg_class c \
                        JOIN pg_namespace n ON n.oid = c.relnamespace \
                        WHERE n.nspname = current_schema() \
                          AND c.relname = 'space_usage_reconcile_jobs' AND c.relkind = 'r' \
                    ) \
                    AND EXISTS ( \
                        SELECT 1 FROM pg_class c \
                        JOIN pg_namespace n ON n.oid = c.relnamespace \
                        WHERE n.nspname = current_schema() \
                          AND c.relname = 'space_usage_reconcile_executions' AND c.relkind = 'r' \
                    ) \
                    AND EXISTS ( \
                        SELECT 1 FROM pg_trigger t \
                        JOIN pg_class c ON c.oid = t.tgrelid \
                        JOIN pg_namespace n ON n.oid = c.relnamespace \
                        WHERE n.nspname = current_schema() AND c.relname = 'spaces' \
                          AND t.tgname = 'spaces_create_usage' \
                          AND NOT t.tgisinternal \
                          AND t.tgenabled IN ('O', 'A') \
                    )",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        if !installed {
            return Err(Error::internal(
                "required space usage schema is not installed",
            ));
        }
        Ok(())
    }

    /// Return whether any live Space is missing its authoritative counter row.
    pub async fn has_missing_live_counters(&self) -> Result<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS ( \
                 SELECT 1 FROM spaces s \
                 WHERE s.deleted_at IS NULL \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM space_usage su WHERE su.space_id = s.id \
                   ) \
             )",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    /// Calculate source-of-truth usage for diagnostics and reconciliation.
    /// Quota checks must use the locked counter instead of this full scan.
    pub async fn calculate_exact_usage(&self, space_id: Uuid) -> Result<UsageCounts> {
        let mut connection = self.pool.acquire().await.map_err(map_sqlx_error)?;
        exact_usage(&mut connection, space_id).await
    }

    /// Execute at most one queued Space reconciliation.
    pub async fn execute_next_reconciliation(&self) -> Result<UsageReconcileExecution> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        configure_transaction(&mut tx, RECONCILE_STATEMENT_TIMEOUT).await?;

        if !try_acquire_worker_lock(&mut tx).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(UsageReconcileExecution::WorkerLockHeld);
        }
        delete_expired_executions(&mut tx).await?;

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

    /// Rebuild every live Space counter in one maintenance transaction.
    pub async fn execute_full_recalculation(&self) -> Result<FullUsageReconcileExecution> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        configure_transaction(&mut tx, FULL_RECALCULATION_STATEMENT_TIMEOUT).await?;

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

    let previous = sqlx::query_as::<_, UsageCounts>(
        "SELECT live_node_count, live_content_bytes \
         FROM space_usage WHERE space_id = $1 FOR UPDATE",
    )
    .bind(job.space_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::internal("live space is missing its usage counter"))?;
    let actual = exact_usage(tx, job.space_id).await?;

    sqlx::query(
        "UPDATE space_usage \
         SET live_node_count = $2, live_content_bytes = $3, reconciled_at = now() \
         WHERE space_id = $1",
    )
    .bind(job.space_id)
    .bind(actual.live_node_count)
    .bind(actual.live_content_bytes)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    insert_execution(
        tx,
        job,
        "succeeded",
        None,
        json!({
            "previous_nodes": previous.live_node_count,
            "actual_nodes": actual.live_node_count,
            "previous_content_bytes": previous.live_content_bytes,
            "actual_content_bytes": actual.live_content_bytes,
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

async fn delete_expired_executions(tx: &mut sqlx::PgConnection) -> Result<()> {
    sqlx::query(
        "DELETE FROM space_usage_reconcile_executions \
         WHERE finished_at < now() - interval '3 months'",
    )
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

async fn configure_transaction(tx: &mut sqlx::PgConnection, statement_timeout: &str) -> Result<()> {
    sqlx::query(
        "SELECT set_config('lock_timeout', $1, true), \
                set_config('statement_timeout', $2, true)",
    )
    .bind(LOCK_TIMEOUT)
    .bind(statement_timeout)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

async fn try_acquire_worker_lock(tx: &mut sqlx::PgConnection) -> Result<bool> {
    space_usage::try_schema_advisory_lock(tx, RECONCILE_ADVISORY_LOCK_SEED, false).await
}

async fn exact_usage(tx: &mut sqlx::PgConnection, space_id: Uuid) -> Result<UsageCounts> {
    sqlx::query_as(
        "SELECT \
             (SELECT count(*) FROM nodes \
              WHERE space_id = $1 AND deleted_at IS NULL) AS live_node_count, \
             COALESCE(( \
                 SELECT sum(t.byte_len) FROM text_objects t \
                 JOIN nodes n ON n.id = t.node_id AND n.space_id = t.space_id \
                 WHERE t.space_id = $1 AND n.deleted_at IS NULL \
             ), 0)::bigint + COALESCE(( \
                 SELECT sum(f.byte_len) FROM file_objects f \
                 JOIN nodes n ON n.id = f.node_id AND n.space_id = f.space_id \
                 WHERE f.space_id = $1 AND n.deleted_at IS NULL \
             ), 0)::bigint AS live_content_bytes",
    )
    .bind(space_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)
}

#[derive(Debug, Clone, FromRow, PartialEq, Eq)]
struct QueuedJob {
    job_id: Uuid,
    space_id: Uuid,
    started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, FromRow, PartialEq, Eq)]
pub struct UsageCounts {
    pub live_node_count: i64,
    pub live_content_bytes: i64,
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
        previous: UsageCounts,
        actual: UsageCounts,
    },
    Failed {
        job_id: Uuid,
        space_id: Uuid,
        run_after: DateTime<Utc>,
        error: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullUsageReconcileExecution {
    WorkerLockHeld,
    MutationsActive,
    Recalculated { spaces_recalculated: u64 },
}
