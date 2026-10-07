//! User-facing usage snapshots and manual Space reconciliation requests.

use chrono::{DateTime, Duration, Utc};
use notegate_core::tier::UserTier;
use notegate_core::{Error, Result};
use notegate_jobs::{JobHistoryContext, JobQueue, NewJob};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    SpaceUsagePayload, SpaceUsageReconcileJob, active_account_predicate, map_sqlx_error, to_usize,
};

const MANUAL_RECONCILE_COOLDOWN_SECONDS: i64 = 60 * 60;
const REQUEST_LOCK_TIMEOUT: &str = "1s";
const REQUEST_RETRY_AFTER_SECONDS: u64 = 2;

#[derive(Debug, Clone)]
pub struct UsageRepo {
    pool: PgPool,
}

impl UsageRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn current_user_usage(&self, user_id: Uuid) -> Result<Option<UserUsageSnapshot>> {
        let active_user = active_account_predicate("acc.");
        let user = sqlx::query_as::<_, UserUsageRow>(sqlx::AssertSqlSafe(format!(
            "SELECT u.tier \
             FROM users u \
             JOIN accounts acc ON acc.id = u.id \
             WHERE u.id = $1 AND acc.kind = 'user' AND {active_user}"
        )))
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        let Some(user) = user else {
            return Ok(None);
        };

        let rows = sqlx::query_as::<_, SpaceUsageRow>(
            "WITH owned AS ( \
                 SELECT id, name, sort_order, deleted_at IS NOT NULL AS deleted, false AS removed \
                 FROM spaces WHERE owner_user_id = $1 \
                 UNION ALL \
                 SELECT u.space_id, 'Deleted space', 0, true, true FROM space_storage_usage u \
                 WHERE u.owner_user_id = $1 AND (u.text_bytes > 0 OR u.file_bytes > 0) \
                   AND NOT EXISTS (SELECT 1 FROM spaces s WHERE s.id = u.space_id) \
             ) SELECT s.id, s.name, s.deleted, \
                    CASE WHEN s.removed THEN 1 ELSE su.live_node_count END AS live_node_count, \
                    CASE WHEN s.deleted THEN 0 ELSE su.live_text_bytes END AS live_text_bytes, \
                    CASE WHEN s.deleted THEN 0 ELSE su.live_file_bytes END AS live_file_bytes, \
                    stored.text_bytes AS stored_text_bytes, stored.file_bytes AS stored_file_bytes, \
                    COALESCE(su.reconciled_at, 'epoch'::timestamptz) AS reconciled_at, \
                    NOT s.deleted AND EXISTS ( \
                        SELECT 1 FROM background_jobs job \
                        WHERE job.job_kind = 'space_usage_reconcile' \
                          AND job.status IN ('queued', 'running') \
                          AND job.payload ->> 'space_id' = s.id::text \
                    ) AS reconciliation_pending \
             FROM owned s LEFT JOIN space_storage_usage stored ON stored.space_id = s.id \
             LEFT JOIN space_usage su ON su.space_id = s.id \
             ORDER BY s.sort_order, s.name, s.id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let spaces = rows
            .into_iter()
            .map(SpaceUsageSnapshot::try_from)
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(UserUsageSnapshot {
            tier: UserTier::parse_db(&user.tier)?,
            spaces,
        }))
    }

    pub async fn request_space_reconciliation(
        &self,
        owner_user_id: Uuid,
        space_id: Uuid,
    ) -> Result<UsageReconciliationOutcome> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        sqlx::query("SELECT set_config('lock_timeout', $1, true)")
            .bind(REQUEST_LOCK_TIMEOUT)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        // Match file mutations and the reconciler: Space row before usage row.
        let live_space = sqlx::query_as::<_, ReconcileSpaceRow>(
            "SELECT id, name FROM spaces \
             WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL \
             FOR UPDATE",
        )
        .bind(space_id)
        .bind(owner_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_request_lock_error)?;
        let live_space = live_space.ok_or_else(|| Error::not_found("space not found"))?;

        let state = sqlx::query_as::<_, ReconcileRequestRow>(
            "SELECT su.reconciled_at, now() AS requested_at, \
                    EXISTS (SELECT 1 FROM background_jobs job \
                        WHERE job.job_kind = 'space_usage_reconcile' \
                          AND job.status IN ('queued', 'running') \
                          AND job.payload ->> 'space_id' = su.space_id::text) AS pending \
             FROM space_usage su WHERE su.space_id = $1 FOR UPDATE",
        )
        .bind(space_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_request_lock_error)?
        .ok_or_else(|| Error::internal("live space is missing its usage counter"))?;

        let no_queue_outcome = if state.pending {
            Some(UsageReconciliationOutcome::AlreadyQueued)
        } else if state.reconciled_at
            > state.requested_at - Duration::seconds(MANUAL_RECONCILE_COOLDOWN_SECONDS)
        {
            Some(UsageReconciliationOutcome::Cooldown)
        } else {
            None
        };
        if let Some(outcome) = no_queue_outcome {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(outcome);
        }

        JobQueue::enqueue_in(
            &mut tx,
            &NewJob::<SpaceUsageReconcileJob>::new(SpaceUsagePayload { space_id })
                .record_in_history(
                    owner_user_id,
                    Some(
                        JobHistoryContext::new("space")
                            .id(live_space.id)
                            .label(live_space.name),
                    ),
                ),
        )
        .await
        .map_err(|error| Error::internal(error.to_string()))?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(UsageReconciliationOutcome::Queued)
    }
}

fn map_request_lock_error(error: sqlx::Error) -> Error {
    if let sqlx::Error::Database(database_error) = &error
        && database_error.code().as_deref() == Some("55P03")
    {
        return Error::usage_recalculation_in_progress(REQUEST_RETRY_AFTER_SECONDS);
    }
    map_sqlx_error(error)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageReconciliationOutcome {
    Queued,
    AlreadyQueued,
    Cooldown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserUsageSnapshot {
    pub tier: UserTier,
    pub spaces: Vec<SpaceUsageSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceUsageSnapshot {
    pub id: Uuid,
    pub name: String,
    pub deleted: bool,
    pub live_nodes: usize,
    pub live_text_bytes: usize,
    pub live_file_bytes: usize,
    pub stored_text_bytes: usize,
    pub stored_file_bytes: usize,
    pub reconciliation_pending: bool,
    pub reconciliation_available_at: DateTime<Utc>,
}

impl TryFrom<SpaceUsageRow> for SpaceUsageSnapshot {
    type Error = Error;

    fn try_from(row: SpaceUsageRow) -> Result<Self> {
        let missing_counter = || Error::internal("live space is missing its usage counter");
        let live_node_count = row.live_node_count.ok_or_else(missing_counter)?;
        let live_text_bytes = row.live_text_bytes.ok_or_else(missing_counter)?;
        let live_file_bytes = row.live_file_bytes.ok_or_else(missing_counter)?;
        let stored_text_bytes = row.stored_text_bytes.ok_or_else(missing_counter)?;
        let stored_file_bytes = row.stored_file_bytes.ok_or_else(missing_counter)?;
        let reconciled_at = row.reconciled_at.ok_or_else(missing_counter)?;
        Ok(Self {
            id: row.id,
            name: row.name,
            deleted: row.deleted,
            live_nodes: to_usize(live_node_count, "node")?,
            live_text_bytes: to_usize(live_text_bytes, "text byte")?,
            live_file_bytes: to_usize(live_file_bytes, "file byte")?,
            stored_text_bytes: to_usize(stored_text_bytes, "stored text byte")?,
            stored_file_bytes: to_usize(stored_file_bytes, "stored file byte")?,
            reconciliation_pending: row.reconciliation_pending,
            reconciliation_available_at: reconciled_at
                + Duration::seconds(MANUAL_RECONCILE_COOLDOWN_SECONDS),
        })
    }
}

#[derive(Debug, FromRow)]
struct UserUsageRow {
    tier: String,
}

#[derive(Debug, FromRow)]
struct SpaceUsageRow {
    id: Uuid,
    name: String,
    deleted: bool,
    live_node_count: Option<i64>,
    live_text_bytes: Option<i64>,
    live_file_bytes: Option<i64>,
    stored_text_bytes: Option<i64>,
    stored_file_bytes: Option<i64>,
    reconciled_at: Option<DateTime<Utc>>,
    reconciliation_pending: bool,
}

#[derive(Debug, FromRow)]
struct ReconcileRequestRow {
    reconciled_at: DateTime<Utc>,
    requested_at: DateTime<Utc>,
    pending: bool,
}

#[derive(Debug, FromRow)]
struct ReconcileSpaceRow {
    id: Uuid,
    name: String,
}
