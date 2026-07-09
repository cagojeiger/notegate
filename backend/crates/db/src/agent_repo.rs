//! Agent account persistence.
//!
//! All queries use runtime-checked `query_as::<_, Row>()` / `query()` — never
//! the `query!` macro. Agent creation inserts `accounts(kind='agent')` then the
//! `agents` row in one transaction, attributing `owner_user_id` to the caller. API
//! keys are persisted by `ApiKeyRepo`, not this aggregate repository.

use crate::audit_events::{self, AuditContext};
use crate::{active_account_predicate, map_sqlx_error, tier_lookup};
use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use notegate_model::CreateAgent;
use notegate_model::account::{Account, AccountKind};
use notegate_model::agent::Agent;
use serde_json::json;

use sqlx::{FromRow, PgPool, Row as _};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AgentRepo {
    pool: PgPool,
}

impl AgentRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// A row from `accounts`. Agent display names are derived from `agents.name`.
#[derive(Debug, FromRow)]
struct AccountRow {
    id: Uuid,
    kind: String,
    is_active: bool,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl AccountRow {
    fn into_agent_account(self, display_name: String) -> Result<Account> {
        let kind = AccountKind::parse(&self.kind)
            .ok_or_else(|| Error::internal(format!("unknown account kind: {}", self.kind)))?;
        Ok(Account {
            id: self.id,
            kind,
            display_name,
            is_active: self.is_active,
            deleted_at: self.deleted_at,
            deleted_by: self.deleted_by,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// A row from `agents`.
#[derive(Debug, FromRow)]
struct AgentRow {
    id: Uuid,
    name: String,
    owner_user_id: Uuid,
}

impl From<AgentRow> for Agent {
    fn from(row: AgentRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            owner_user_id: row.owner_user_id,
        }
    }
}

const ACCOUNT_COLUMNS: &str =
    "id, kind, is_active, deleted_at, deleted_by_account_id AS deleted_by, created_at, updated_at";
const AGENT_COLUMNS: &str = "id, name, owner_user_id";
impl AgentRepo {
    /// Soft-deactivate an agent account and revoke its non-revoked keys and
    /// access in one transaction.
    pub async fn delete_agent(&self, agent_id: Uuid, deleted_by: Uuid) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let owner_user_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT a.owner_user_id FROM agents a \
             JOIN accounts acc ON acc.id = a.id \
             WHERE a.id = $1 AND acc.kind = 'agent' AND acc.deleted_at IS NULL \
             FOR UPDATE OF acc",
        )
        .bind(agent_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let Some(owner_user_id) = owner_user_id else {
            return Err(Error::not_found("agent not found"));
        };

        let result = sqlx::query(
            "UPDATE accounts \
             SET is_active = false, deleted_at = now(), deleted_by_account_id = $2, updated_at = now() \
             WHERE id = $1 AND kind = 'agent' AND deleted_at IS NULL",
        )
        .bind(agent_id)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("agent not found"));
        }

        let revoked_keys = sqlx::query(
            "UPDATE api_keys SET revoked_at = now(), revoked_by_user_id = $2 \
             WHERE account_id = $1 AND revoked_at IS NULL",
        )
        .bind(agent_id)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let disconnected_connections = sqlx::query(
            "UPDATE space_agent_connections SET disconnected_at = now(), disconnected_by_user_id = $2 \
             WHERE agent_id = $1 AND disconnected_at IS NULL",
        )
        .bind(agent_id)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let audit_ctx = AuditContext::rest(deleted_by);
        audit_events::record(
            &mut tx,
            audit_ctx,
            owner_user_id,
            "agent.delete",
            "agent",
            Some(agent_id),
            json!({
                "revoked_agent_keys": revoked_keys.rows_affected(),
                "disconnected_connections": disconnected_connections.rows_affected(),
            }),
        )
        .await?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }
}

impl AgentRepo {
    pub async fn insert_agent(&self, command: &CreateAgent, owner_user_id: Uuid) -> Result<Agent> {
        validate_agent_name(&command.name)?;
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let creator_tier = tier_lookup::lock_active_user_tier(
            &mut tx,
            owner_user_id,
            "agent creator user account not found",
        )
        .await?;
        let quota = creator_tier.quota();

        let active: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM agents a \
             JOIN accounts acc ON acc.id = a.id \
             WHERE a.owner_user_id = $1 AND acc.is_active = true AND acc.deleted_at IS NULL",
        )
        .bind(owner_user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let active =
            usize::try_from(active).map_err(|_error| Error::internal("negative agent count"))?;
        if active >= quota.agents_per_user {
            return Err(Error::conflict(format!(
                "creator already has the maximum of {} active agents for tier {}",
                quota.agents_per_user,
                creator_tier.as_str()
            )));
        }

        let id: Uuid = sqlx::query("INSERT INTO accounts (kind) VALUES ('agent') RETURNING id")
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?
            .get::<Uuid, _>("id");

        let row = sqlx::query_as::<_, AgentRow>(&format!(
            "INSERT INTO agents (id, name, owner_user_id) VALUES ($1, $2, $3) \
             RETURNING {AGENT_COLUMNS}"
        ))
        .bind(id)
        .bind(&command.name)
        .bind(owner_user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let audit_ctx = AuditContext::rest(owner_user_id);
        audit_events::record(
            &mut tx,
            audit_ctx,
            owner_user_id,
            "agent.create",
            "agent",
            Some(row.id),
            json!({}),
        )
        .await?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(Agent::from(row))
    }

    pub async fn list_agents_by_creator(&self, creator_account_id: Uuid) -> Result<Vec<Agent>> {
        let rows = sqlx::query_as::<_, AgentRow>(&format!(
            "SELECT a.{cols} FROM agents a \
             JOIN accounts acc ON acc.id = a.id \
             WHERE a.owner_user_id = $1 AND acc.is_active = true AND acc.deleted_at IS NULL \
             ORDER BY acc.created_at, a.id",
            cols = "id, name, owner_user_id"
        ))
        .bind(creator_account_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(rows.into_iter().map(Agent::from).collect())
    }

    pub async fn count_agents_by_creator(&self, creator_account_id: Uuid) -> Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM agents a \
             JOIN accounts acc ON acc.id = a.id \
             WHERE a.owner_user_id = $1 AND acc.is_active = true AND acc.deleted_at IS NULL",
        )
        .bind(creator_account_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        usize::try_from(count).map_err(|_error| Error::internal("negative agent count"))
    }

    pub async fn find_active_agent_by_creator(
        &self,
        agent_id: Uuid,
        creator_account_id: Uuid,
    ) -> Result<Option<Agent>> {
        let row = sqlx::query_as::<_, AgentRow>(&format!(
            "SELECT a.{cols} FROM agents a \
             JOIN accounts acc ON acc.id = a.id \
             WHERE a.id = $1 AND a.owner_user_id = $2 \
               AND acc.is_active = true AND acc.deleted_at IS NULL",
            cols = "id, name, owner_user_id"
        ))
        .bind(agent_id)
        .bind(creator_account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(Agent::from))
    }
}

fn validate_agent_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(Error::validation("agent name cannot be empty"));
    }
    if name.chars().count() > limits::AGENT_NAME_MAX_CHARS {
        return Err(Error::validation(format!(
            "agent name exceeds the maximum of {} characters",
            limits::AGENT_NAME_MAX_CHARS
        )));
    }
    Ok(())
}

impl AgentRepo {
    pub async fn find_active_agent_by_id(
        &self,
        agent_id: Uuid,
    ) -> Result<Option<(Account, Agent)>> {
        let account_row = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM accounts \
             WHERE id = $1 AND kind = 'agent' AND {}",
            active_account_predicate("")
        ))
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let Some(account_row) = account_row else {
            return Ok(None);
        };

        let agent_row = sqlx::query_as::<_, AgentRow>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE id = $1"
        ))
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let Some(agent_row) = agent_row else {
            return Ok(None);
        };

        let agent = Agent::from(agent_row);
        let account = account_row.into_agent_account(agent.name.clone())?;
        Ok(Some((account, agent)))
    }
}
