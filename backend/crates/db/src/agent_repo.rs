//! Agents + agent_keys persistence.
//!
//! All queries use runtime-checked `query_as::<_, Row>()` / `query()` — never
//! the `query!` macro. Agent creation inserts `accounts(kind='agent')` then the
//! `agents` row in one transaction, attributing `created_by` to the caller. Key
//! authentication matches on `token_hash` only and rejects revoked, expired, or
//! inactive credentials.

use chrono::{DateTime, Utc};
use notegate_core::{Error, Result};
use notegate_model::account::{Account, AccountKind};
use notegate_model::agent::{Agent, AgentKey};
use notegate_service::agents::{AgentStore, CreateAgent, CreateAgentKey};
use notegate_service::identity::AgentAuthStore;
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

/// A row from `accounts`.
#[derive(Debug, FromRow)]
struct AccountRow {
    id: Uuid,
    kind: String,
    display_name: String,
    is_active: bool,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl AccountRow {
    fn into_account(self) -> Result<Account> {
        let kind = AccountKind::parse(&self.kind)
            .ok_or_else(|| Error::internal(format!("unknown account kind: {}", self.kind)))?;
        Ok(Account {
            id: self.id,
            kind,
            display_name: self.display_name,
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

/// A row from `agent_keys`.
#[derive(Debug, FromRow)]
struct AgentKeyRow {
    id: Uuid,
    agent_id: Uuid,
    token_hash: String,
    name: String,
    scopes: Vec<String>,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    revoked_by: Option<Uuid>,
}

impl From<AgentKeyRow> for AgentKey {
    fn from(row: AgentKeyRow) -> Self {
        Self {
            id: row.id,
            agent_id: row.agent_id,
            token_hash: row.token_hash,
            name: row.name,
            scopes: row.scopes,
            created_by: row.created_by,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
            expires_at: row.expires_at,
            revoked_at: row.revoked_at,
            revoked_by: row.revoked_by,
        }
    }
}

const ACCOUNT_COLUMNS: &str =
    "id, kind, display_name, is_active, deleted_at, deleted_by, created_at, updated_at";
const AGENT_COLUMNS: &str = "id, name, created_by";
const AGENT_KEY_COLUMNS: &str = "id, agent_id, token_hash, name, scopes, created_by, \
     created_at, last_used_at, expires_at, revoked_at, revoked_by";

impl AgentRepo {
    /// Revoke an agent key, recording who revoked it. The key must belong to
    /// `agent_id`; otherwise it is reported as not-found so a caller cannot
    /// revoke a key from another agent by guessing its id.
    pub async fn revoke_key(&self, agent_id: Uuid, key_id: Uuid, revoked_by: Uuid) -> Result<()> {
        let result = sqlx::query(
            "UPDATE agent_keys \
             SET revoked_at = now(), revoked_by = $2 \
             WHERE id = $1 AND agent_id = $3 AND revoked_at IS NULL",
        )
        .bind(key_id)
        .bind(revoked_by)
        .bind(agent_id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("agent key not found"));
        }
        Ok(())
    }

    /// Load an agent's detail by id (regardless of account state).
    pub async fn find_agent(&self, agent_id: Uuid) -> Result<Option<Agent>> {
        let row = sqlx::query_as::<_, AgentRow>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE id = $1"
        ))
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(Agent::from))
    }

    /// Soft-deactivate an agent account and revoke its non-revoked keys and
    /// access in one transaction.
    pub async fn delete_agent(&self, agent_id: Uuid, deleted_by: Uuid) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE accounts \
             SET is_active = false, deleted_at = now(), deleted_by = $2, updated_at = now() \
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(agent_id)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE agent_keys SET revoked_at = now(), revoked_by = $2 \
             WHERE agent_id = $1 AND revoked_at IS NULL",
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

impl AgentStore for AgentRepo {
    async fn insert_agent(&self, command: &CreateAgent, created_by: Uuid) -> Result<Agent> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let id: Uuid = sqlx::query(
            "INSERT INTO accounts (kind, display_name) VALUES ('agent', $1) RETURNING id",
        )
        .bind(&command.name)
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

    async fn list_agents_by_creator(&self, creator_account_id: Uuid) -> Result<Vec<Agent>> {
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

    async fn count_agents_by_creator(&self, creator_account_id: Uuid) -> Result<usize> {
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

    async fn find_active_agent_by_creator(
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

    async fn insert_agent_key(
        &self,
        command: &CreateAgentKey,
        token_hash: &str,
        created_by: Uuid,
    ) -> Result<AgentKey> {
        let row = sqlx::query_as::<_, AgentKeyRow>(&format!(
            "INSERT INTO agent_keys (agent_id, token_hash, name, scopes, created_by, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING {AGENT_KEY_COLUMNS}"
        ))
        .bind(command.agent_id)
        .bind(token_hash)
        .bind(&command.name)
        .bind(&command.scopes)
        .bind(created_by)
        .bind(command.expires_at)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(AgentKey::from(row))
    }

    async fn count_live_keys(&self, agent_id: Uuid) -> Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM agent_keys \
             WHERE agent_id = $1 AND revoked_at IS NULL \
               AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(agent_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        usize::try_from(count).map_err(|_error| Error::internal("negative key count"))
    }

    async fn delete_agent(&self, agent_id: Uuid, deleted_by: Uuid) -> Result<()> {
        AgentRepo::delete_agent(self, agent_id, deleted_by).await
    }

    async fn revoke_key(&self, agent_id: Uuid, key_id: Uuid, revoked_by: Uuid) -> Result<()> {
        AgentRepo::revoke_key(self, agent_id, key_id, revoked_by).await
    }
}

impl AgentAuthStore for AgentRepo {
    async fn find_agent_by_key_hash(&self, token_hash: &str) -> Result<Option<(Account, Agent)>> {
        // Reject revoked, expired, and inactive credentials at the SQL layer so
        // a stale key never resolves to a caller.
        let agent_id: Option<Uuid> = sqlx::query(
            "SELECT k.agent_id FROM agent_keys k \
             JOIN accounts acc ON acc.id = k.agent_id \
             WHERE k.token_hash = $1 \
               AND k.revoked_at IS NULL \
               AND (k.expires_at IS NULL OR k.expires_at > now()) \
               AND acc.is_active = true \
               AND acc.deleted_at IS NULL",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| row.get::<Uuid, _>("agent_id"));

        let Some(agent_id) = agent_id else {
            return Ok(None);
        };

        // Record last use; failure here must not block authentication.
        if let Err(error) =
            sqlx::query("UPDATE agent_keys SET last_used_at = now() WHERE token_hash = $1")
                .bind(token_hash)
                .execute(&self.pool)
                .await
        {
            tracing::warn!(event = "agent_key.last_used_update_failed", %error);
        }

        let account_row = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = $1"
        ))
        .bind(agent_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        let agent_row = sqlx::query_as::<_, AgentRow>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE id = $1"
        ))
        .bind(agent_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(Some((account_row.into_account()?, Agent::from(agent_row))))
    }
}

fn map_sqlx_error(error: sqlx::Error) -> Error {
    Error::internal(format!("agent repository query failed: {error}"))
}
