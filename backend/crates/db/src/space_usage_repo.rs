//! Exact reconciliation for transactionally maintained Space usage counters.

use chrono::{DateTime, Utc};
use notegate_core::Result;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{map_sqlx_error, space_usage};

const RECONCILE_ADVISORY_LOCK_KEY: i64 = 0x4e47_5553_4147_4501;
const DUE_CANDIDATE_LIMIT: i64 = 64;

#[derive(Debug, Clone)]
pub struct SpaceUsageRepo {
    pool: PgPool,
}

impl SpaceUsageRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Reconcile at most one due Space.
    ///
    /// A database-wide advisory lock elects one active worker. Due candidates
    /// whose Space gate is busy are skipped without waiting.
    pub async fn run_reconciliation_once(&self) -> Result<UsageReconcileRun> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        if !try_acquire_worker_lock(&mut tx).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(UsageReconcileRun::LockHeld);
        }

        let candidates = due_candidates(&mut tx).await?;
        let Some(oldest) = candidates.first().cloned() else {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(UsageReconcileRun::Idle);
        };

        let mut selected = None;
        for candidate in &candidates {
            if space_usage::try_acquire_reconciliation_gate(&mut tx, candidate.space_id).await? {
                selected = Some(candidate.clone());
                break;
            }
        }
        let Some(candidate) = selected else {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(UsageReconcileRun::SpacesBusy {
                oldest_space_id: oldest.space_id,
                oldest_due_at: oldest.due_at,
                candidates_checked: candidates.len(),
            });
        };

        let live_space: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM spaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(candidate.space_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if live_space.is_none() {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(UsageReconcileRun::Idle);
        }

        let Some(previous) = sqlx::query_as::<_, UsageCounts>(
            "SELECT live_node_count, live_content_bytes \
             FROM space_usage WHERE space_id = $1 FOR UPDATE",
        )
        .bind(candidate.space_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        else {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(UsageReconcileRun::Idle);
        };

        let actual = exact_usage(&mut tx, candidate.space_id).await?;
        let next_reconcile_at: DateTime<Utc> = sqlx::query_scalar(
            "UPDATE space_usage \
             SET live_node_count = $2, live_content_bytes = $3, reconciled_at = now(), \
                 next_reconcile_at = now() + interval '7 days' \
                     + ((random() - 0.5) * interval '24 hours') \
             WHERE space_id = $1 \
             RETURNING next_reconcile_at",
        )
        .bind(candidate.space_id)
        .bind(actual.live_node_count)
        .bind(actual.live_content_bytes)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(UsageReconcileRun::Reconciled {
            space_id: candidate.space_id,
            due_at: candidate.due_at,
            previous,
            actual,
            next_reconcile_at,
        })
    }

    /// Rebuild every live Space counter in one maintenance transaction.
    ///
    /// The worker lock excludes scheduled reconciliation. The global exclusive
    /// gate rejects new file-tree mutations while the source rows are scanned;
    /// reads remain available throughout the run.
    pub async fn run_full_recalculation(&self) -> Result<FullUsageReconcileRun> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        if !try_acquire_worker_lock(&mut tx).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(FullUsageReconcileRun::WorkerLockHeld);
        }
        if !space_usage::try_acquire_full_reconciliation_gate(&mut tx).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(FullUsageReconcileRun::MutationsActive);
        }

        let result = sqlx::query(
            "INSERT INTO space_usage (\
                 space_id, live_node_count, live_content_bytes, reconciled_at, next_reconcile_at\
             ) \
             SELECT \
                 s.id, \
                 count(n.id) FILTER (WHERE n.deleted_at IS NULL)::bigint, \
                 COALESCE(sum(t.byte_len) FILTER (WHERE n.deleted_at IS NULL), 0)::bigint + \
                 COALESCE(sum(f.byte_len) FILTER (WHERE n.deleted_at IS NULL), 0)::bigint, \
                 now(), \
                 now() + interval '7 days' + ((random() - 0.5) * interval '24 hours') \
             FROM spaces s \
             LEFT JOIN nodes n ON n.space_id = s.id \
             LEFT JOIN text_objects t ON t.space_id = n.space_id AND t.node_id = n.id \
             LEFT JOIN file_objects f ON f.space_id = n.space_id AND f.node_id = n.id \
             WHERE s.deleted_at IS NULL \
             GROUP BY s.id \
             ON CONFLICT (space_id) DO UPDATE \
             SET live_node_count = EXCLUDED.live_node_count, \
                 live_content_bytes = EXCLUDED.live_content_bytes, \
                 reconciled_at = EXCLUDED.reconciled_at, \
                 next_reconcile_at = EXCLUDED.next_reconcile_at",
        )
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(FullUsageReconcileRun::Recalculated {
            spaces_recalculated: result.rows_affected(),
        })
    }
}

async fn try_acquire_worker_lock(tx: &mut sqlx::PgConnection) -> Result<bool> {
    sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(RECONCILE_ADVISORY_LOCK_KEY)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)
}

async fn due_candidates(tx: &mut sqlx::PgConnection) -> Result<Vec<DueSpace>> {
    sqlx::query_as(
        "SELECT su.space_id, su.next_reconcile_at AS due_at \
         FROM space_usage su \
         JOIN spaces s ON s.id = su.space_id \
         WHERE s.deleted_at IS NULL AND su.next_reconcile_at <= now() \
         ORDER BY su.next_reconcile_at, su.space_id \
         LIMIT $1",
    )
    .bind(DUE_CANDIDATE_LIMIT)
    .fetch_all(&mut *tx)
    .await
    .map_err(map_sqlx_error)
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
struct DueSpace {
    space_id: Uuid,
    due_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, FromRow, PartialEq, Eq)]
pub struct UsageCounts {
    pub live_node_count: i64,
    pub live_content_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageReconcileRun {
    LockHeld,
    Idle,
    SpacesBusy {
        oldest_space_id: Uuid,
        oldest_due_at: DateTime<Utc>,
        candidates_checked: usize,
    },
    Reconciled {
        space_id: Uuid,
        due_at: DateTime<Utc>,
        previous: UsageCounts,
        actual: UsageCounts,
        next_reconcile_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullUsageReconcileRun {
    WorkerLockHeld,
    MutationsActive,
    Recalculated { spaces_recalculated: u64 },
}
