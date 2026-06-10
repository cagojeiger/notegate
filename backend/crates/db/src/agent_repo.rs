//! Agent account persistence.
//!
//! All queries use runtime-checked `query_as::<_, Row>()` / `query()` — never
//! the `query!` macro. Agent creation inserts `accounts(kind='agent')` then the
//! `agents` row in one transaction, attributing `created_by` to the caller. API
//! keys are persisted by `ApiKeyRepo`, not this aggregate repository.

use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use notegate_model::CreateAgent;
use notegate_model::account::{Account, AccountKind};
use notegate_model::agent::Agent;

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
    created_by: Uuid,
}

impl From<AgentRow> for Agent {
    fn from(row: AgentRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            created_by: row.created_by,
        }
    }
}

const ACCOUNT_COLUMNS: &str = "id, kind, is_active, deleted_at, deleted_by, created_at, updated_at";
const AGENT_COLUMNS: &str = "id, name, created_by";
impl AgentRepo {
    /// Soft-deactivate an agent account and revoke its non-revoked keys and
    /// access in one transaction.
    pub async fn delete_agent(&self, agent_id: Uuid, deleted_by: Uuid) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let result = sqlx::query(
            "UPDATE accounts \
             SET is_active = false, deleted_at = now(), deleted_by = $2, updated_at = now() \
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

        sqlx::query(
            "UPDATE api_keys SET revoked_at = now(), revoked_by = $2 \
             WHERE account_id = $1 AND revoked_at IS NULL",
        )
        .bind(agent_id)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE workspace_access SET revoked_at = now(), revoked_by = $2 \
             WHERE account_id = $1 AND revoked_at IS NULL",
        )
        .bind(agent_id)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }
}

impl AgentRepo {
    pub async fn insert_agent(&self, command: &CreateAgent, created_by: Uuid) -> Result<Agent> {
        validate_agent_name(&command.name)?;
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let creator_exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM accounts \
             WHERE id = $1 AND kind = 'user' AND is_active = true AND deleted_at IS NULL \
             FOR UPDATE",
        )
        .bind(created_by)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if creator_exists.is_none() {
            return Err(Error::not_found("agent creator user account not found"));
        }

        let active: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM agents a \
             JOIN accounts acc ON acc.id = a.id \
             WHERE a.created_by = $1 AND acc.is_active = true AND acc.deleted_at IS NULL",
        )
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let active =
            usize::try_from(active).map_err(|_error| Error::internal("negative agent count"))?;
        if active >= limits::AGENTS_PER_CREATOR_MAX {
            return Err(Error::conflict(format!(
                "creator already has the maximum of {} active agents",
                limits::AGENTS_PER_CREATOR_MAX
            )));
        }

        let id: Uuid = sqlx::query("INSERT INTO accounts (kind) VALUES ('agent') RETURNING id")
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?
            .get::<Uuid, _>("id");

        let row = sqlx::query_as::<_, AgentRow>(&format!(
            "INSERT INTO agents (id, name, created_by) VALUES ($1, $2, $3) \
             RETURNING {AGENT_COLUMNS}"
        ))
        .bind(id)
        .bind(&command.name)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(Agent::from(row))
    }

    pub async fn list_agents_by_creator(&self, creator_account_id: Uuid) -> Result<Vec<Agent>> {
        let rows = sqlx::query_as::<_, AgentRow>(&format!(
            "SELECT a.{cols} FROM agents a \
             JOIN accounts acc ON acc.id = a.id \
             WHERE a.created_by = $1 AND acc.is_active = true AND acc.deleted_at IS NULL \
             ORDER BY acc.created_at, a.id",
            cols = "id, name, created_by"
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
             WHERE a.created_by = $1 AND acc.is_active = true AND acc.deleted_at IS NULL",
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
             WHERE a.id = $1 AND a.created_by = $2 \
               AND acc.is_active = true AND acc.deleted_at IS NULL",
            cols = "id, name, created_by"
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
             WHERE id = $1 AND kind = 'agent' \
               AND is_active = true AND deleted_at IS NULL"
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
