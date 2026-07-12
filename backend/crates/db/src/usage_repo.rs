//! User-facing usage snapshots and manual Space reconciliation requests.

use chrono::{DateTime, Duration, Utc};
use notegate_core::tier::UserTier;
use notegate_core::{Error, Result};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{active_account_predicate, map_sqlx_error, space_usage, to_usize};

const MANUAL_RECONCILE_COOLDOWN_SECONDS: i64 = 60 * 60;

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
        let account = sqlx::query_as::<_, UserUsageRow>(&format!(
            "SELECT u.tier, \
                    (SELECT count(*) FROM agents a \
                     JOIN accounts agent_acc ON agent_acc.id = a.id \
                     WHERE a.owner_user_id = u.id \
                       AND agent_acc.is_active = true AND agent_acc.deleted_at IS NULL) AS live_agents, \
                    (SELECT count(*) FROM api_keys k \
                     WHERE k.account_id = u.id AND k.revoked_at IS NULL AND k.expires_at > now()) AS live_api_keys \
             FROM users u \
             JOIN accounts acc ON acc.id = u.id \
             WHERE u.id = $1 AND acc.kind = 'user' AND {active_user}"
        ))
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        let Some(account) = account else {
            return Ok(None);
        };

        let rows = sqlx::query_as::<_, SpaceUsageRow>(
            "SELECT s.id, s.name, su.live_node_count, su.live_content_bytes, \
                    su.reconciled_at, \
                    EXISTS ( \
                        SELECT 1 FROM space_usage_reconcile_jobs j WHERE j.space_id = s.id \
                    ) AS reconciliation_pending, \
                    (SELECT count(*) FROM space_agent_connections c \
                     JOIN accounts agent_acc ON agent_acc.id = c.agent_id \
                     WHERE c.space_id = s.id AND c.disconnected_at IS NULL \
                       AND agent_acc.is_active = true AND agent_acc.deleted_at IS NULL) \
                        AS live_agent_connections \
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
            tier: UserTier::parse_db(&account.tier)?,
            live_agents: to_usize(account.live_agents, "agent")?,
            live_api_keys: to_usize(account.live_api_keys, "api key")?,
            spaces,
        }))
    }

    pub async fn request_space_reconciliation(
        &self,
        owner_user_id: Uuid,
        space_id: Uuid,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        space_usage::acquire_maintenance_gate(&mut tx).await?;
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
        .map_err(map_sqlx_error)?;
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
        .map_err(map_sqlx_error)?
        .ok_or_else(|| Error::internal("live space is missing its usage counter"))?;

        if state.pending {
            return Err(Error::conflict(
                "space usage reconciliation is already queued",
            ));
        }
        if state.reconciled_at
            > state.requested_at - Duration::seconds(MANUAL_RECONCILE_COOLDOWN_SECONDS)
        {
            return Err(Error::conflict(
                "space usage was reconciled recently; try again later",
            ));
        }

        sqlx::query("INSERT INTO space_usage_reconcile_jobs (space_id) VALUES ($1)")
            .bind(space_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserUsageSnapshot {
    pub tier: UserTier,
    pub live_agents: usize,
    pub live_api_keys: usize,
    pub spaces: Vec<SpaceUsageSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceUsageSnapshot {
    pub id: Uuid,
    pub name: String,
    pub live_nodes: usize,
    pub live_content_bytes: usize,
    pub live_agent_connections: usize,
    pub reconciled_at: DateTime<Utc>,
    pub reconciliation_pending: bool,
}

impl TryFrom<SpaceUsageRow> for SpaceUsageSnapshot {
    type Error = Error;

    fn try_from(row: SpaceUsageRow) -> Result<Self> {
        let missing_counter = || Error::internal("live space is missing its usage counter");
        let live_node_count = row.live_node_count.ok_or_else(missing_counter)?;
        let live_content_bytes = row.live_content_bytes.ok_or_else(missing_counter)?;
        Ok(Self {
            id: row.id,
            name: row.name,
            live_nodes: to_usize(live_node_count, "node")?,
            live_content_bytes: to_usize(live_content_bytes, "content byte")?,
            live_agent_connections: to_usize(row.live_agent_connections, "connection")?,
            reconciled_at: row.reconciled_at.ok_or_else(missing_counter)?,
            reconciliation_pending: row.reconciliation_pending.ok_or_else(missing_counter)?,
        })
    }
}

#[derive(Debug, FromRow)]
struct UserUsageRow {
    tier: String,
    live_agents: i64,
    live_api_keys: i64,
}

#[derive(Debug, FromRow)]
struct SpaceUsageRow {
    id: Uuid,
    name: String,
    live_node_count: Option<i64>,
    live_content_bytes: Option<i64>,
    reconciled_at: Option<DateTime<Utc>>,
    reconciliation_pending: Option<bool>,
    live_agent_connections: i64,
}

#[derive(Debug, FromRow)]
struct ReconcileRequestRow {
    reconciled_at: DateTime<Utc>,
    requested_at: DateTime<Utc>,
    pending: bool,
}
