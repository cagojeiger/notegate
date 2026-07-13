//! User-facing usage snapshots and manual Space reconciliation requests.

use chrono::{DateTime, Duration, Utc};
use notegate_core::tier::UserTier;
use notegate_core::{Error, Result};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{active_account_predicate, map_sqlx_error, to_usize};

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
        let user = sqlx::query_as::<_, UserUsageRow>(&format!(
            "SELECT u.tier \
             FROM users u \
             JOIN accounts acc ON acc.id = u.id \
             WHERE u.id = $1 AND acc.kind = 'user' AND {active_user}"
        ))
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        let Some(user) = user else {
            return Ok(None);
        };

        let rows = sqlx::query_as::<_, SpaceUsageRow>(
            "SELECT s.id, s.name, su.live_node_count, su.live_text_bytes, su.live_file_bytes, \
                    EXISTS ( \
                        SELECT 1 FROM space_usage_reconcile_jobs j WHERE j.space_id = s.id \
                    ) AS reconciliation_pending \
             FROM spaces s \
             LEFT JOIN space_usage su ON su.space_id = s.id \
             WHERE s.owner_user_id = $1 AND s.deleted_at IS NULL \
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
        let live_space: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM spaces \
             WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL \
             FOR UPDATE",
        )
        .bind(space_id)
        .bind(owner_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_request_lock_error)?;
        if live_space.is_none() {
            return Err(Error::not_found("space not found"));
        }

        let state = sqlx::query_as::<_, ReconcileRequestRow>(
            "SELECT su.reconciled_at, now() AS requested_at, \
                    EXISTS ( \
                        SELECT 1 FROM space_usage_reconcile_jobs j WHERE j.space_id = su.space_id \
                    ) AS pending \
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

        sqlx::query("INSERT INTO space_usage_reconcile_jobs (space_id) VALUES ($1)")
            .bind(space_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
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
    pub live_nodes: usize,
    pub live_text_bytes: usize,
    pub live_file_bytes: usize,
    pub reconciliation_pending: bool,
}

impl TryFrom<SpaceUsageRow> for SpaceUsageSnapshot {
    type Error = Error;

    fn try_from(row: SpaceUsageRow) -> Result<Self> {
        let missing_counter = || Error::internal("live space is missing its usage counter");
        let live_node_count = row.live_node_count.ok_or_else(missing_counter)?;
        let live_text_bytes = row.live_text_bytes.ok_or_else(missing_counter)?;
        let live_file_bytes = row.live_file_bytes.ok_or_else(missing_counter)?;
        Ok(Self {
            id: row.id,
            name: row.name,
            live_nodes: to_usize(live_node_count, "node")?,
            live_text_bytes: to_usize(live_text_bytes, "text byte")?,
            live_file_bytes: to_usize(live_file_bytes, "file byte")?,
            reconciliation_pending: row.reconciliation_pending,
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
    live_node_count: Option<i64>,
    live_text_bytes: Option<i64>,
    live_file_bytes: Option<i64>,
    reconciliation_pending: bool,
}

#[derive(Debug, FromRow)]
struct ReconcileRequestRow {
    reconciled_at: DateTime<Utc>,
    requested_at: DateTime<Utc>,
    pending: bool,
}
