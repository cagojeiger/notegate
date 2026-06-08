//! Agent lifecycle: create / delete agents and their keys.
//!
//! POLICY: only `kind='user'` callers may manage agents or keys; an agent caller
//! is forbidden. Active-agent and live-key counts are enforced against
//! `core::limits` before each insert; the database repo repeats these caps
//! inside transactions for real persistence. The plaintext key token is
//! generated here, hashed with SHA-256, and only the hash is persisted.

use chrono::Utc;
use notegate_core::limits;
use notegate_db::AgentRepo;
use notegate_model::Agent;
use notegate_model::account::AccountKind;
pub use notegate_model::{AgentPage, CreateAgent, CreateAgentKey, ListAgents, MintedAgentKey};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::pagination::{clamp_limit, paginate_by_id};

/// Agent lifecycle service.
#[derive(Debug, Clone)]
pub struct AgentService {
    store: AgentRepo,
}

impl AgentService {
    pub fn new(store: AgentRepo) -> Self {
        Self { store }
    }

    /// Create an agent. Only a `kind='user'` caller may create agents; the
    /// user caller may create at most [`limits::AGENTS_PER_CREATOR_MAX`] active agents.
    pub async fn create_agent(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        command: CreateAgent,
    ) -> ServiceResult<Agent> {
        require_user_caller(caller_kind)?;
        validate_agent_name(&command.name)?;

        let active = self
            .store
            .count_agents_by_creator(caller_account_id)
            .await?;
        if active >= limits::AGENTS_PER_CREATOR_MAX {
            return Err(ServiceError::Conflict(format!(
                "creator already has the maximum of {} active agents",
                limits::AGENTS_PER_CREATOR_MAX
            )));
        }

        Ok(self.store.insert_agent(&command, caller_account_id).await?)
    }

    /// List all active agents created by the caller. Only user callers may manage agents.
    pub async fn list_agents(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
    ) -> ServiceResult<Vec<Agent>> {
        require_user_caller(caller_kind)?;
        Ok(self.store.list_agents_by_creator(caller_account_id).await?)
    }

    /// List active agents created by the caller, paginated with an opaque cursor.
    /// Only user callers may manage agents.
    pub async fn list_agents_page(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        request: ListAgents,
    ) -> ServiceResult<AgentPage> {
        require_user_caller(caller_kind)?;
        let limit = clamp_limit(
            request.limit,
            limits::AGENTS_DEFAULT_LIMIT,
            limits::AGENTS_MAX_LIMIT,
        );
        let agents = self.store.list_agents_by_creator(caller_account_id).await?;
        let (items, has_more, next_cursor) =
            paginate_by_id(agents, |agent| agent.id, limit, request.cursor.as_deref())?;
        Ok(AgentPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    /// Create an agent key. Only a `kind='user'` caller may create keys; the
    /// agent may have at most [`limits::AGENT_KEYS_PER_AGENT_MAX`] live keys.
    pub async fn create_key(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        command: CreateAgentKey,
    ) -> ServiceResult<MintedAgentKey> {
        require_user_caller(caller_kind)?;
        if !command.scopes.is_empty() {
            return Err(ServiceError::InvalidInput(
                "agent key scopes must be empty".to_owned(),
            ));
        }
        if command
            .expires_at
            .is_some_and(|expires_at| expires_at <= Utc::now())
        {
            return Err(ServiceError::InvalidInput(
                "agent key expires_at must be in the future".to_owned(),
            ));
        }
        self.require_owned_active_agent(command.agent_id, caller_account_id)
            .await?;

        let live = self.store.count_live_keys(command.agent_id).await?;
        if live >= limits::AGENT_KEYS_PER_AGENT_MAX {
            return Err(ServiceError::Conflict(format!(
                "agent already has the maximum of {} live keys",
                limits::AGENT_KEYS_PER_AGENT_MAX
            )));
        }

        let token = generate_token();
        let token_hash = hash_token(&token);
        let key = self
            .store
            .insert_agent_key(&command, &token_hash, caller_account_id)
            .await?;
        Ok(MintedAgentKey { key, token })
    }

    /// Delete an agent created by the caller. Only user callers may manage
    /// agents; missing, inactive, or differently-owned agents are hidden as 404.
    pub async fn delete_agent(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        agent_id: Uuid,
    ) -> ServiceResult<()> {
        require_user_caller(caller_kind)?;
        self.require_owned_active_agent(agent_id, caller_account_id)
            .await?;
        Ok(self.store.delete_agent(agent_id, caller_account_id).await?)
    }

    /// Revoke one key from an agent created by the caller.
    pub async fn revoke_key(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        agent_id: Uuid,
        key_id: Uuid,
    ) -> ServiceResult<()> {
        require_user_caller(caller_kind)?;
        self.require_owned_active_agent(agent_id, caller_account_id)
            .await?;
        Ok(self
            .store
            .revoke_key(agent_id, key_id, caller_account_id)
            .await?)
    }

    async fn require_owned_active_agent(
        &self,
        agent_id: Uuid,
        creator_account_id: Uuid,
    ) -> ServiceResult<Agent> {
        self.store
            .find_active_agent_by_creator(agent_id, creator_account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("agent not found".to_owned()))
    }
}

/// Reject any caller that is not a user account.
fn require_user_caller(kind: AccountKind) -> ServiceResult<()> {
    match kind {
        AccountKind::User => Ok(()),
        AccountKind::Agent => Err(ServiceError::Forbidden(
            "only user accounts may manage agents".to_owned(),
        )),
    }
}

fn validate_agent_name(name: &str) -> ServiceResult<()> {
    if name.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "agent name cannot be empty".to_owned(),
        ));
    }
    Ok(())
}

/// Generate a random opaque token (256 bits of entropy, hex-encoded).
fn generate_token() -> String {
    use rand::RngCore as _;
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Hash a plaintext token for storage/lookup. Shared by minting and auth so the
/// stored hash and the lookup hash never drift.
pub fn hash_token(token: &str) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
