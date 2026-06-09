//! Agents + unified api_keys persistence.
//!
//! All queries use runtime-checked `query_as::<_, Row>()` / `query()` — never
//! the `query!` macro. Agent creation inserts `accounts(kind='agent')` then the
//! `agents` row in one transaction, attributing `created_by` to the caller. Key
//! authentication matches on `token_hash` only and rejects revoked, expired, or
//! inactive credentials.

use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use notegate_model::account::{Account, AccountKind};
use notegate_model::agent::{Agent, AgentKey};
use notegate_model::{CreateAgent, CreateAgentKey};

use sqlx::{FromRow, PgPool, Row as _};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AgentRepo {
    pool: PgPool,
    lookup_key_id: String,
    hash_version: i32,
}

impl AgentRepo {
    pub fn new(pool: PgPool) -> Self {
        Self::with_lookup_key(pool, "test-lookup", 1)
    }

    pub fn with_lookup_key(
        pool: PgPool,
        lookup_key_id: impl Into<String>,
        hash_version: i32,
    ) -> Self {
        Self {
            pool,
            lookup_key_id: lookup_key_id.into(),
            hash_version,
        }
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

/// A row from `api_keys` projected for agent-key responses.
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

const ACCOUNT_COLUMNS: &str = "id, kind, is_active, deleted_at, deleted_by, created_at, updated_at";
const AGENT_COLUMNS: &str = "id, name, created_by";
const AGENT_KEY_COLUMNS: &str = "id, account_id AS agent_id, token_hash, name, scopes, created_by, \
     created_at, last_used_at, expires_at, revoked_at, revoked_by";

impl AgentRepo {
    /// Revoke an agent key, recording who revoked it. The key must belong to
    /// `agent_id`; otherwise it is reported as not-found so a caller cannot
    /// revoke a key from another agent by guessing its id.
    pub async fn revoke_key(&self, agent_id: Uuid, key_id: Uuid, revoked_by: Uuid) -> Result<()> {
        let result = sqlx::query(
            "UPDATE api_keys \
             SET revoked_at = now(), revoked_by = $2 \
             WHERE id = $1 AND account_id = $3 AND revoked_at IS NULL",
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

    pub async fn insert_agent_key(
        &self,
        command: &CreateAgentKey,
        token_hash: &str,
        created_by: Uuid,
    ) -> Result<AgentKey> {
        let key_id = Uuid::new_v4();
        let token_prefix: String = token_hash.chars().take(12).collect();
        self.insert_agent_key_with_id(key_id, command, &token_prefix, token_hash, created_by)
            .await
    }

    pub async fn insert_agent_key_with_id(
        &self,
        key_id: Uuid,
        command: &CreateAgentKey,
        token_prefix: &str,
        token_hash: &str,
        created_by: Uuid,
    ) -> Result<AgentKey> {
        if !command.scopes.is_empty() {
            return Err(Error::validation("api key scopes must be empty"));
        }

        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let agent_exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT a.id FROM agents a \
             JOIN accounts agent_acc ON agent_acc.id = a.id \
             JOIN accounts creator_acc ON creator_acc.id = a.created_by \
             WHERE a.id = $1 AND a.created_by = $2 \
               AND agent_acc.is_active = true AND agent_acc.deleted_at IS NULL \
               AND creator_acc.kind = 'user' \
               AND creator_acc.is_active = true AND creator_acc.deleted_at IS NULL \
             FOR UPDATE OF agent_acc",
        )
        .bind(command.agent_id)
        .bind(created_by)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if agent_exists.is_none() {
            return Err(Error::not_found("agent not found"));
        }

        let live: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM api_keys \
             WHERE account_id = $1 AND revoked_at IS NULL \
               AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(command.agent_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let live = usize::try_from(live).map_err(|_error| Error::internal("negative key count"))?;
        if live >= limits::AGENT_KEYS_PER_AGENT_MAX {
            return Err(Error::conflict(format!(
                "agent already has the maximum of {} live keys",
                limits::AGENT_KEYS_PER_AGENT_MAX
            )));
        }

        let row = sqlx::query_as::<_, AgentKeyRow>(&format!(
            "INSERT INTO api_keys \
             (id, account_id, token_prefix, token_hash, hash_key_id, hash_version, name, scopes, created_by, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING {AGENT_KEY_COLUMNS}"
        ))
        .bind(key_id)
        .bind(command.agent_id)
        .bind(token_prefix)
        .bind(token_hash)
        .bind(&self.lookup_key_id)
        .bind(self.hash_version)
        .bind(&command.name)
        .bind(&command.scopes)
        .bind(created_by)
        .bind(command.expires_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(AgentKey::from(row))
    }

    pub async fn count_live_keys(&self, agent_id: Uuid) -> Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM api_keys \
             WHERE account_id = $1 AND revoked_at IS NULL \
               AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(agent_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        usize::try_from(count).map_err(|_error| Error::internal("negative key count"))
    }
}

impl AgentRepo {
    pub async fn find_agent_by_key_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<(Account, Agent)>> {
        // Reject revoked, expired, and inactive credentials at the SQL layer so
        // a stale key never resolves to a caller.
        let agent_id: Option<Uuid> = sqlx::query(
            "SELECT k.account_id FROM api_keys k \
             JOIN accounts acc ON acc.id = k.account_id \
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
        .map(|row| row.get::<Uuid, _>("account_id"));

        let Some(agent_id) = agent_id else {
            return Ok(None);
        };

        // Record last use; failure here must not block authentication.
        if let Err(error) =
            sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE token_hash = $1")
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

        let agent = Agent::from(agent_row);
        let account = account_row.into_agent_account(agent.name.clone())?;

        Ok(Some((account, agent)))
    }
}
