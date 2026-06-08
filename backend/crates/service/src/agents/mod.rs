//! Agent lifecycle: create / delete agents and their keys.
//!
//! POLICY: only `kind='user'` callers may manage agents or keys; an agent caller
//! is forbidden. Active-agent and live-key counts are enforced against
//! `core::limits` before each insert; the database repo repeats these caps
//! inside transactions for real persistence. The plaintext key token is
//! generated here, hashed with SHA-256, and only the hash is persisted.

use std::future::Future;

use chrono::{DateTime, Utc};
use notegate_core::Result as CoreResult;
use notegate_core::limits;
use notegate_model::account::AccountKind;
use notegate_model::{Agent, AgentKey};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::pagination::{clamp_limit, paginate_by_id};

/// Input to create an agent.
#[derive(Debug, Clone)]
pub struct CreateAgent {
    pub name: String,
}

/// Input to create an agent key.
#[derive(Debug, Clone)]
pub struct CreateAgentKey {
    pub agent_id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Input to list agents created by the caller.
#[derive(Debug, Clone, Default)]
pub struct ListAgents {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// A page of agents.
#[derive(Debug, Clone)]
pub struct AgentPage {
    pub items: Vec<Agent>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// A freshly minted agent key, including the one-time plaintext token.
#[derive(Debug, Clone)]
pub struct MintedAgentKey {
    pub key: AgentKey,
    /// The plaintext token, returned exactly once at creation.
    pub token: String,
}

/// Persistence for agents and agent keys.
pub trait AgentStore: Clone + Send + Sync + 'static {
    /// Insert an agent account + detail.
    fn insert_agent(
        &self,
        command: &CreateAgent,
        created_by: Uuid,
    ) -> impl Future<Output = CoreResult<Agent>> + Send;

    /// List active agents created by a user account.
    fn list_agents_by_creator(
        &self,
        creator_account_id: Uuid,
    ) -> impl Future<Output = CoreResult<Vec<Agent>>> + Send;

    /// Count active agents created by a user account.
    fn count_agents_by_creator(
        &self,
        creator_account_id: Uuid,
    ) -> impl Future<Output = CoreResult<usize>> + Send;

    /// Load an active agent created by the caller. Missing, inactive, or
    /// differently-owned agents are all `None` so callers can hide them as 404.
    fn find_active_agent_by_creator(
        &self,
        agent_id: Uuid,
        creator_account_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<Agent>>> + Send;

    /// Insert an agent key with a pre-computed token hash.
    fn insert_agent_key(
        &self,
        command: &CreateAgentKey,
        token_hash: &str,
        created_by: Uuid,
    ) -> impl Future<Output = CoreResult<AgentKey>> + Send;

    /// Count live keys for an agent.
    fn count_live_keys(&self, agent_id: Uuid) -> impl Future<Output = CoreResult<usize>> + Send;

    /// Soft-delete an owned active agent and revoke its keys/access.
    fn delete_agent(
        &self,
        agent_id: Uuid,
        deleted_by: Uuid,
    ) -> impl Future<Output = CoreResult<()>> + Send;

    /// Revoke a key belonging to an owned active agent.
    fn revoke_key(
        &self,
        agent_id: Uuid,
        key_id: Uuid,
        revoked_by: Uuid,
    ) -> impl Future<Output = CoreResult<()>> + Send;
}

/// Agent lifecycle service.
#[derive(Debug, Clone)]
pub struct AgentService<S> {
    store: S,
}

impl<S> AgentService<S>
where
    S: AgentStore,
{
    pub fn new(store: S) -> Self {
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use super::*;
    use crate::cursor;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockStore {
        agent_count: usize,
        key_count: usize,
        owned_agent_exists: bool,
        agents: Vec<Agent>,
        inserted_agents: Arc<Mutex<Vec<(String, Uuid)>>>,
        deleted_agents: Arc<Mutex<Vec<Uuid>>>,
        revoked_keys: Arc<Mutex<Vec<(Uuid, Uuid)>>>,
    }

    impl Default for MockStore {
        fn default() -> Self {
            Self {
                agent_count: 0,
                key_count: 0,
                owned_agent_exists: true,
                agents: Vec::new(),
                inserted_agents: Arc::new(Mutex::new(Vec::new())),
                deleted_agents: Arc::new(Mutex::new(Vec::new())),
                revoked_keys: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl AgentStore for MockStore {
        async fn insert_agent(&self, command: &CreateAgent, created_by: Uuid) -> CoreResult<Agent> {
            self.inserted_agents
                .lock()
                .map_err(|_error| notegate_core::Error::internal("lock poisoned"))?
                .push((command.name.clone(), created_by));
            Ok(Agent {
                id: Uuid::new_v4(),
                name: command.name.clone(),
                created_by,
            })
        }

        async fn list_agents_by_creator(&self, _creator: Uuid) -> CoreResult<Vec<Agent>> {
            Ok(self.agents.clone())
        }

        async fn count_agents_by_creator(&self, _creator: Uuid) -> CoreResult<usize> {
            Ok(self.agent_count)
        }

        async fn find_active_agent_by_creator(
            &self,
            agent_id: Uuid,
            creator_account_id: Uuid,
        ) -> CoreResult<Option<Agent>> {
            if !self.owned_agent_exists {
                return Ok(None);
            }
            Ok(Some(Agent {
                id: agent_id,
                name: "bot".to_owned(),
                created_by: creator_account_id,
            }))
        }

        async fn insert_agent_key(
            &self,
            command: &CreateAgentKey,
            token_hash: &str,
            created_by: Uuid,
        ) -> CoreResult<AgentKey> {
            Ok(AgentKey {
                id: Uuid::new_v4(),
                agent_id: command.agent_id,
                token_hash: token_hash.to_owned(),
                name: command.name.clone(),
                scopes: command.scopes.clone(),
                created_by: Some(created_by),
                created_at: Utc::now(),
                last_used_at: None,
                expires_at: command.expires_at,
                revoked_at: None,
                revoked_by: None,
            })
        }

        async fn count_live_keys(&self, _agent_id: Uuid) -> CoreResult<usize> {
            Ok(self.key_count)
        }

        async fn delete_agent(&self, agent_id: Uuid, _deleted_by: Uuid) -> CoreResult<()> {
            self.deleted_agents
                .lock()
                .map_err(|_error| notegate_core::Error::internal("lock poisoned"))?
                .push(agent_id);
            Ok(())
        }

        async fn revoke_key(
            &self,
            agent_id: Uuid,
            key_id: Uuid,
            _revoked_by: Uuid,
        ) -> CoreResult<()> {
            self.revoked_keys
                .lock()
                .map_err(|_error| notegate_core::Error::internal("lock poisoned"))?
                .push((agent_id, key_id));
            Ok(())
        }
    }

    #[tokio::test]
    async fn agent_caller_cannot_create_agent() {
        let service = AgentService::new(MockStore::default());
        let err = service
            .create_agent(
                AccountKind::Agent,
                Uuid::new_v4(),
                CreateAgent {
                    name: "bot".to_owned(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden(_)));
    }

    #[tokio::test]
    async fn agent_caller_cannot_list_agents() {
        let service = AgentService::new(MockStore::default());
        let err = service
            .list_agents_page(
                AccountKind::Agent,
                Uuid::new_v4(),
                ListAgents {
                    limit: None,
                    cursor: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden(_)));
    }

    #[tokio::test]
    async fn fifty_first_agent_is_rejected() {
        let store = MockStore {
            agent_count: limits::AGENTS_PER_CREATOR_MAX,
            ..MockStore::default()
        };
        let service = AgentService::new(store);
        let err = service
            .create_agent(
                AccountKind::User,
                Uuid::new_v4(),
                CreateAgent {
                    name: "bot".to_owned(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn fiftieth_agent_is_allowed() {
        let store = MockStore {
            agent_count: limits::AGENTS_PER_CREATOR_MAX - 1,
            ..MockStore::default()
        };
        let service = AgentService::new(store);
        assert!(
            service
                .create_agent(
                    AccountKind::User,
                    Uuid::new_v4(),
                    CreateAgent {
                        name: "bot".to_owned(),
                    },
                )
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn list_agents_page_returns_opaque_cursor() {
        let creator = Uuid::new_v4();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let third = Uuid::new_v4();
        let service = AgentService::new(MockStore {
            agents: vec![
                Agent {
                    id: first,
                    name: "first".to_owned(),
                    created_by: creator,
                },
                Agent {
                    id: second,
                    name: "second".to_owned(),
                    created_by: creator,
                },
                Agent {
                    id: third,
                    name: "third".to_owned(),
                    created_by: creator,
                },
            ],
            ..MockStore::default()
        });

        let first_page = service
            .list_agents_page(
                AccountKind::User,
                creator,
                ListAgents {
                    limit: Some(2),
                    cursor: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(
            first_page
                .items
                .iter()
                .map(|agent| agent.id)
                .collect::<Vec<_>>(),
            vec![first, second]
        );
        assert_eq!(first_page.limit, 2);
        assert!(first_page.has_more);
        let cursor = first_page.next_cursor.expect("next cursor");
        assert_eq!(cursor::decode::<Uuid>(&cursor).unwrap(), second);

        let second_page = service
            .list_agents_page(
                AccountKind::User,
                creator,
                ListAgents {
                    limit: Some(2),
                    cursor: Some(cursor),
                },
            )
            .await
            .unwrap();

        assert_eq!(
            second_page
                .items
                .iter()
                .map(|agent| agent.id)
                .collect::<Vec<_>>(),
            vec![third]
        );
        assert!(!second_page.has_more);
        assert!(second_page.next_cursor.is_none());
    }

    #[tokio::test]
    async fn eleventh_key_is_rejected() {
        let store = MockStore {
            key_count: limits::AGENT_KEYS_PER_AGENT_MAX,
            ..MockStore::default()
        };
        let service = AgentService::new(store);
        let err = service
            .create_key(
                AccountKind::User,
                Uuid::new_v4(),
                CreateAgentKey {
                    agent_id: Uuid::new_v4(),
                    name: "key".to_owned(),
                    scopes: Vec::new(),
                    expires_at: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn agent_caller_cannot_create_key() {
        let service = AgentService::new(MockStore::default());
        let err = service
            .create_key(
                AccountKind::Agent,
                Uuid::new_v4(),
                CreateAgentKey {
                    agent_id: Uuid::new_v4(),
                    name: "key".to_owned(),
                    scopes: Vec::new(),
                    expires_at: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden(_)));
    }

    #[tokio::test]
    async fn agent_caller_cannot_delete_or_revoke_keys() {
        let service = AgentService::new(MockStore::default());
        let caller = Uuid::new_v4();
        let agent_id = Uuid::new_v4();

        let delete = service
            .delete_agent(AccountKind::Agent, caller, agent_id)
            .await
            .unwrap_err();
        assert!(matches!(delete, ServiceError::Forbidden(_)));

        let revoke = service
            .revoke_key(AccountKind::Agent, caller, agent_id, Uuid::new_v4())
            .await
            .unwrap_err();
        assert!(matches!(revoke, ServiceError::Forbidden(_)));
    }

    #[tokio::test]
    async fn non_empty_key_scopes_are_rejected() {
        let service = AgentService::new(MockStore::default());
        let err = service
            .create_key(
                AccountKind::User,
                Uuid::new_v4(),
                CreateAgentKey {
                    agent_id: Uuid::new_v4(),
                    name: "key".to_owned(),
                    scopes: vec!["files:read".to_owned()],
                    expires_at: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn expired_key_creation_is_rejected() {
        let service = AgentService::new(MockStore::default());
        let err = service
            .create_key(
                AccountKind::User,
                Uuid::new_v4(),
                CreateAgentKey {
                    agent_id: Uuid::new_v4(),
                    name: "key".to_owned(),
                    scopes: Vec::new(),
                    expires_at: Some(Utc::now() - chrono::Duration::seconds(1)),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn missing_owned_agent_is_not_found_when_creating_key() {
        let store = MockStore {
            owned_agent_exists: false,
            ..MockStore::default()
        };
        let service = AgentService::new(store);
        let err = service
            .create_key(
                AccountKind::User,
                Uuid::new_v4(),
                CreateAgentKey {
                    agent_id: Uuid::new_v4(),
                    name: "key".to_owned(),
                    scopes: Vec::new(),
                    expires_at: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[tokio::test]
    async fn delete_and_revoke_are_service_operations() {
        let store = MockStore::default();
        let deleted = store.deleted_agents.clone();
        let revoked = store.revoked_keys.clone();
        let service = AgentService::new(store);
        let agent_id = Uuid::new_v4();
        let key_id = Uuid::new_v4();
        let caller = Uuid::new_v4();

        service
            .delete_agent(AccountKind::User, caller, agent_id)
            .await
            .unwrap();
        service
            .revoke_key(AccountKind::User, caller, agent_id, key_id)
            .await
            .unwrap();

        assert_eq!(deleted.lock().unwrap().as_slice(), &[agent_id]);
        assert_eq!(revoked.lock().unwrap().as_slice(), &[(agent_id, key_id)]);
    }

    #[test]
    fn hash_token_is_deterministic_and_hex() {
        let a = hash_token("secret");
        let b = hash_token("secret");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(hash_token("secret"), hash_token("other"));
    }
}
