//! Agent connection persistence.
//!
//! A connection grants one user-owned agent read or write permission to one live
//! space. Users do not appear in this table: the space owner always has write
//! permission through `spaces.owner_user_id`.

use crate::audit_event_repo::insert_audit_event;
use crate::audit_events::{self, AuditContext};
use crate::{map_sqlx_error, tier_lookup};
use chrono::{DateTime, Utc};
use notegate_core::{Error, Result};
use notegate_model::{ConnectAgent, Permission, SpaceAgentConnection};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ConnectionRepo {
    pool: PgPool,
}

impl ConnectionRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Debug, FromRow)]
struct ConnectionRow {
    space_id: Uuid,
    agent_id: Uuid,
    permission: String,
    connected_by_user_id: Uuid,
    connected_at: DateTime<Utc>,
    disconnected_at: Option<DateTime<Utc>>,
    disconnected_by_user_id: Option<Uuid>,
}

impl ConnectionRow {
    fn into_connection(self) -> Result<SpaceAgentConnection> {
        let permission = Permission::parse(&self.permission).ok_or_else(|| {
            Error::internal(format!(
                "unknown space connection permission: {}",
                self.permission
            ))
        })?;
        Ok(SpaceAgentConnection {
            space_id: self.space_id,
            agent_id: self.agent_id,
            permission,
            connected_by_user_id: self.connected_by_user_id,
            connected_at: self.connected_at,
            disconnected_at: self.disconnected_at,
            disconnected_by_user_id: self.disconnected_by_user_id,
        })
    }
}

const CONNECTION_COLUMNS: &str = "space_id, agent_id, permission, connected_by_user_id, connected_at, disconnected_at, disconnected_by_user_id";

impl ConnectionRepo {
    pub async fn list_connections(
        &self,
        space_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<Vec<SpaceAgentConnection>> {
        require_owned_space(&self.pool, space_id, owner_user_id).await?;
        let rows = sqlx::query_as::<_, ConnectionRow>(&format!(
            "SELECT c.{CONNECTION_COLUMNS} \
             FROM space_agent_connections c \
             JOIN spaces s ON s.id = c.space_id AND s.deleted_at IS NULL AND s.owner_user_id = $2 \
             JOIN agents a ON a.id = c.agent_id \
             JOIN accounts acc ON acc.id = a.id AND acc.is_active = true AND acc.deleted_at IS NULL \
             WHERE c.space_id = $1 AND c.disconnected_at IS NULL \
             ORDER BY c.connected_at, c.agent_id"
        ))
        .bind(space_id)
        .bind(owner_user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.into_iter()
            .map(ConnectionRow::into_connection)
            .collect()
    }

    pub async fn upsert_connection(
        &self,
        command: &ConnectAgent,
        connected_by_user_id: Uuid,
    ) -> Result<SpaceAgentConnection> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        lock_owned_space(&mut tx, command.space_id, connected_by_user_id).await?;
        lock_owned_live_agent(&mut tx, command.agent_id, connected_by_user_id).await?;
        let owner_tier = tier_lookup::lock_active_user_tier(
            &mut tx,
            connected_by_user_id,
            "user account not found",
        )
        .await?;
        let quota = owner_tier.quota();

        let active_connections: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM space_agent_connections c \
             JOIN agents a ON a.id = c.agent_id \
             JOIN accounts acc ON acc.id = a.id \
             WHERE c.space_id = $1 AND c.agent_id <> $2 AND c.disconnected_at IS NULL \
               AND acc.is_active = true AND acc.deleted_at IS NULL",
        )
        .bind(command.space_id)
        .bind(command.agent_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let active_connections = usize::try_from(active_connections)
            .map_err(|_| Error::internal("negative connection count"))?;
        if active_connections >= quota.connections_per_space {
            return Err(Error::conflict(format!(
                "space already has the maximum of {} active agent connections for tier {}",
                quota.connections_per_space,
                owner_tier.as_str()
            )));
        }

        let connected_spaces: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM space_agent_connections c \
             JOIN spaces s ON s.id = c.space_id AND s.deleted_at IS NULL \
             WHERE c.agent_id = $1 AND c.space_id <> $2 AND c.disconnected_at IS NULL",
        )
        .bind(command.agent_id)
        .bind(command.space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let connected_spaces = usize::try_from(connected_spaces)
            .map_err(|_| Error::internal("negative connected space count"))?;
        if connected_spaces >= quota.connected_spaces_per_agent {
            return Err(Error::conflict(format!(
                "agent is already connected to the maximum of {} spaces for tier {}",
                quota.connected_spaces_per_agent,
                owner_tier.as_str()
            )));
        }

        let row = sqlx::query_as::<_, ConnectionRow>(&format!(
            "INSERT INTO space_agent_connections \
               (space_id, agent_id, permission, connected_by_user_id) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (space_id, agent_id) DO UPDATE \
             SET permission = EXCLUDED.permission, \
                 connected_by_user_id = EXCLUDED.connected_by_user_id, \
                 connected_at = now(), \
                 disconnected_at = NULL, \
                 disconnected_by_user_id = NULL \
             RETURNING {CONNECTION_COLUMNS}"
        ))
        .bind(command.space_id)
        .bind(command.agent_id)
        .bind(command.permission.as_str())
        .bind(connected_by_user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let audit_ctx = AuditContext::rest(connected_by_user_id);
        insert_audit_event(
            &mut tx,
            audit_events::connection_upserted(
                audit_ctx,
                connected_by_user_id,
                command.space_id,
                command.agent_id,
                command.permission.as_str(),
            ),
        )
        .await?;

        tx.commit().await.map_err(map_sqlx_error)?;
        row.into_connection()
    }

    pub async fn disconnect(
        &self,
        space_id: Uuid,
        agent_id: Uuid,
        disconnected_by_user_id: Uuid,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        lock_owned_space(&mut tx, space_id, disconnected_by_user_id).await?;
        lock_owned_live_agent(&mut tx, agent_id, disconnected_by_user_id).await?;

        let result = sqlx::query(
            "UPDATE space_agent_connections \
             SET disconnected_at = now(), disconnected_by_user_id = $3 \
             WHERE space_id = $1 AND agent_id = $2 AND disconnected_at IS NULL",
        )
        .bind(space_id)
        .bind(agent_id)
        .bind(disconnected_by_user_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() > 0 {
            let audit_ctx = AuditContext::rest(disconnected_by_user_id);
            insert_audit_event(
                &mut tx,
                audit_events::connection_disconnected(
                    audit_ctx,
                    disconnected_by_user_id,
                    space_id,
                    agent_id,
                ),
            )
            .await?;
        }

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }
}

async fn lock_owned_space(
    tx: &mut sqlx::PgConnection,
    space_id: Uuid,
    owner_user_id: Uuid,
) -> Result<()> {
    let found: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM spaces \
         WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL \
         FOR UPDATE",
    )
    .bind(space_id)
    .bind(owner_user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    found
        .map(|_| ())
        .ok_or_else(|| Error::not_found("space not found"))
}

async fn lock_owned_live_agent(
    tx: &mut sqlx::PgConnection,
    agent_id: Uuid,
    owner_user_id: Uuid,
) -> Result<()> {
    let found: Option<Uuid> = sqlx::query_scalar(
        "SELECT a.id FROM agents a \
         JOIN accounts acc ON acc.id = a.id \
         WHERE a.id = $1 AND a.owner_user_id = $2 \
           AND acc.kind = 'agent' AND acc.is_active = true AND acc.deleted_at IS NULL \
         FOR UPDATE OF acc",
    )
    .bind(agent_id)
    .bind(owner_user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    found
        .map(|_| ())
        .ok_or_else(|| Error::not_found("agent not found"))
}

async fn require_owned_space(pool: &PgPool, space_id: Uuid, owner_user_id: Uuid) -> Result<()> {
    let found: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM spaces \
         WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL",
    )
    .bind(space_id)
    .bind(owner_user_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;
    found
        .map(|_| ())
        .ok_or_else(|| Error::not_found("space not found"))
}
