//! Identity resolution: turning verified credentials into a [`Caller`].
//!
//! The [`Resolver`] is the single place where verified credentials become a
//! [`Caller`]:
//!
//! - browser login (OAuth callback) creates or updates a user account;
//! - browser session cookies resolve an already-registered user account on the
//!   browser channel;
//! - REST/MCP bearer tokens resolve an already-registered user account
//!   (an authenticated authgate identity with no local account is
//!   [`IdentityError::NotRegistered`] — the spec onboarding path);
//! - an API key resolves either a `kind='user'` or `kind='agent'` account,
//!   rejecting revoked, expired, or inactive credentials (enforced at the db layer).

use notegate_core::security::PiiCrypto;
use notegate_db::{AccountRepo, AgentRepo, ApiKeyRepo};
pub use notegate_model::ResolveAttrs;
use notegate_model::account::AccountKind;
use notegate_model::{Account, Caller, CallerIdentity, Channel, User};

/// Why caller resolution failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    /// The credential is valid but maps to no local account.
    #[error("caller not registered")]
    NotRegistered,
    /// The local account exists but is deactivated.
    #[error("caller account is inactive")]
    Inactive,
    /// An internal/storage failure during resolution.
    #[error("identity resolution failed: {0}")]
    Internal(String),
}

impl From<notegate_core::Error> for IdentityError {
    fn from(error: notegate_core::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

/// Resolves verified credentials into an authenticated [`Caller`].
#[derive(Debug, Clone)]
pub struct Resolver {
    users: AccountRepo,
    agents: AgentRepo,
    api_keys: ApiKeyRepo,
    crypto: PiiCrypto,
}

impl Resolver {
    pub fn new(
        users: AccountRepo,
        agents: AgentRepo,
        api_keys: ApiKeyRepo,
        crypto: PiiCrypto,
    ) -> Self {
        Self {
            users,
            agents,
            api_keys,
            crypto,
        }
    }

    /// Resolve a browser login: create or update the user account, then return
    /// the caller on the browser channel. Inactive accounts remain rejected.
    pub async fn resolve_browser(&self, attrs: ResolveAttrs) -> Result<Caller, IdentityError> {
        let (account, user) = self.users.upsert_user_by_sub(&attrs).await?;
        user_caller(account, user, Channel::Browser)
    }

    /// Resolve a browser session cookie for an already-registered user account.
    pub async fn resolve_browser_session(&self, sub: &str) -> Result<Caller, IdentityError> {
        self.resolve_registered_user(sub, Channel::Browser).await
    }

    /// Resolve a REST bearer for an already-registered user account.
    pub async fn resolve_api(&self, attrs: ResolveAttrs) -> Result<Caller, IdentityError> {
        self.resolve_registered_user(&attrs.sub, Channel::Api).await
    }

    /// Resolve an MCP bearer for an already-registered user account.
    pub async fn resolve_mcp(&self, attrs: ResolveAttrs) -> Result<Caller, IdentityError> {
        self.resolve_registered_user(&attrs.sub, Channel::Mcp).await
    }

    /// Resolve an API key into a user or agent caller on the given channel.
    pub async fn resolve_api_key(
        &self,
        token: &str,
        channel: Channel,
    ) -> Result<Caller, IdentityError> {
        let Some((key_id, secret)) = crate::api_keys::parse_token(token) else {
            return Err(IdentityError::NotRegistered);
        };
        let token_hash = self.crypto.api_key_hash(&key_id.to_string(), secret)?;
        let account_id = self
            .api_keys
            .find_live_account_id_by_key(key_id, &token_hash)
            .await?
            .ok_or(IdentityError::NotRegistered)?;

        if let Some((account, user)) = self.users.find_caller_by_account_id(account_id).await? {
            if account.kind != AccountKind::User {
                return Err(IdentityError::Inactive);
            }
            return user_caller(account, user, channel);
        }

        let resolved = self.agents.find_active_agent_by_id(account_id).await?;
        let (account, agent) = resolved.ok_or(IdentityError::NotRegistered)?;
        if account.kind != AccountKind::Agent {
            return Err(IdentityError::Inactive);
        }

        Ok(Caller {
            account,
            identity: CallerIdentity::Agent(agent),
            channel,
        })
    }

    async fn resolve_registered_user(
        &self,
        sub: &str,
        channel: Channel,
    ) -> Result<Caller, IdentityError> {
        let resolved = self.users.find_user_by_sub(sub).await?;
        let (account, user) = resolved.ok_or(IdentityError::NotRegistered)?;
        user_caller(account, user, channel)
    }
}

/// Build a user caller, rejecting an inactive account.
fn user_caller(account: Account, user: User, channel: Channel) -> Result<Caller, IdentityError> {
    if !account.is_active {
        return Err(IdentityError::Inactive);
    }
    Ok(Caller {
        account,
        identity: CallerIdentity::User(user),
        channel,
    })
}
