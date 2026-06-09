//! Agent lifecycle: create / delete agents and their keys.
//!
//! POLICY: only `kind='user'` callers may manage agents or keys; an agent caller
//! is forbidden. Active-agent and live-key counts are enforced before insert.
//! Plaintext API key tokens are HMAC-hashed with the LOOKUP root subkey; only
//! the hash is persisted.

use chrono::Utc;
use notegate_core::{limits, security::PiiCrypto};
use notegate_db::{AgentRepo, ApiKeyRepo};
use notegate_model::Agent;
use notegate_model::account::AccountKind;
pub use notegate_model::{
    AgentPage, ApiKey, CreateAgent, CreateAgentKey, CreateApiKey, ListAgents, MintedApiKey,
};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::pagination::{clamp_limit, paginate_by_id};

/// Agent lifecycle service.
#[derive(Debug, Clone)]
pub struct AgentService {
    store: AgentRepo,
    api_keys: ApiKeyRepo,
    crypto: PiiCrypto,
}

impl AgentService {
    pub fn with_crypto(store: AgentRepo, api_keys: ApiKeyRepo, crypto: PiiCrypto) -> Self {
        Self {
            store,
            api_keys,
            crypto,
        }
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

    /// Create an agent-bound API key. Only a `kind='user'` caller may create keys;
    /// the agent account may have at most [`limits::API_KEYS_PER_ACCOUNT_MAX`] live keys.
    pub async fn create_key(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        command: CreateAgentKey,
    ) -> ServiceResult<MintedApiKey> {
        require_user_caller(caller_kind)?;
        if !command.scopes.is_empty() {
            return Err(ServiceError::InvalidInput(
                "api key scopes must be empty".to_owned(),
            ));
        }
        if command
            .expires_at
            .is_some_and(|expires_at| expires_at <= Utc::now())
        {
            return Err(ServiceError::InvalidInput(
                "api key expires_at must be in the future".to_owned(),
            ));
        }
        self.require_owned_active_agent(command.agent_id, caller_account_id)
            .await?;

        crate::accounts::create_key_for_account(
            &self.api_keys,
            &self.crypto,
            command.agent_id,
            caller_account_id,
            CreateApiKey {
                name: command.name,
                scopes: command.scopes,
                expires_at: command.expires_at,
            },
            None,
        )
        .await
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
            .api_keys
            .revoke_key(agent_id, key_id, caller_account_id, None)
            .await?)
    }

    pub async fn list_keys(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        agent_id: Uuid,
    ) -> ServiceResult<Vec<ApiKey>> {
        require_user_caller(caller_kind)?;
        self.require_owned_active_agent(agent_id, caller_account_id)
            .await?;
        Ok(self.api_keys.list_by_account(agent_id).await?)
    }

    pub async fn rotate_key(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        agent_id: Uuid,
        key_id: Uuid,
    ) -> ServiceResult<MintedApiKey> {
        require_user_caller(caller_kind)?;
        self.require_owned_active_agent(agent_id, caller_account_id)
            .await?;
        let old = self
            .api_keys
            .find_live_key(agent_id, key_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("api key not found".to_owned()))?;
        crate::accounts::rotate_key_for_account(
            &self.api_keys,
            &self.crypto,
            agent_id,
            caller_account_id,
            key_id,
            CreateApiKey {
                name: old.name,
                scopes: Vec::new(),
                expires_at: old.expires_at,
            },
        )
        .await
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

pub fn format_token(key_id: Uuid, secret: &str) -> String {
    format!("ngk_v1_{key_id}_{secret}")
}

pub fn token_prefix(key_id: Uuid) -> String {
    format!("ngk_v1_{key_id}")
}

pub fn parse_token(token: &str) -> Option<(Uuid, &str)> {
    let rest = token.strip_prefix("ngk_v1_")?;
    let (key_id, secret) = rest.split_once('_')?;
    let key_id = Uuid::parse_str(key_id).ok()?;
    if secret.is_empty() {
        return None;
    }
    Some((key_id, secret))
}

#[cfg(test)]
mod api_key_token_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn api_key_token_round_trips_key_id_and_secret() {
        let key_id = Uuid::new_v4();
        let token = format_token(key_id, "secret-value");
        let parsed = parse_token(&token).unwrap();
        assert_eq!(parsed.0, key_id);
        assert_eq!(parsed.1, "secret-value");
        assert_eq!(token_prefix(key_id), format!("ngk_v1_{key_id}"));
    }

    #[test]
    fn api_key_token_rejects_old_opaque_tokens() {
        assert!(parse_token("old-token").is_none());
        assert!(parse_token("ngk_v1_not-a-uuid_secret").is_none());
        assert!(parse_token("ngk_v1_00000000-0000-0000-0000-000000000000_").is_none());
    }
}
