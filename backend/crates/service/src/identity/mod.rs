//! Identity resolution: turning verified credentials into a [`Caller`].
//!
//! The [`Resolver`] is the single place where verified credentials become a
//! [`Caller`]:
//!
//! - browser login (OAuth callback) creates or updates a user account;
//! - browser session cookies resolve an already-registered user account on the
//!   browser channel;
//! - MCP OAuth bearer tokens resolve an already-registered user account
//!   (an authenticated authgate identity with no local account is
//!   [`IdentityError::NotRegistered`] — the spec onboarding path);
//! - Command API OAuth bearer tokens resolve an already-registered user on the
//!   API channel;
//! - an Agent API key resolves an active `kind='agent'` account, rejecting
//!   revoked, expired, user-owned, or inactive credentials.

use notegate_core::security::PiiCrypto;
use notegate_db::{AccountRepo, ApiKeyRepo};
pub use notegate_model::ResolveAttrs;
use notegate_model::account::AccountKind;
use notegate_model::{Account, Caller, CallerIdentity, Channel, User};
use uuid::Uuid;

/// Why caller resolution failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    /// The credential is valid but maps to no local account.
    #[error("caller not registered")]
    NotRegistered,
    /// The local account exists but is deactivated.
    #[error("caller account is inactive")]
    Inactive,
    /// The verified identity attributes violate notegate input limits.
    #[error("invalid identity attributes")]
    InvalidInput,
    /// An internal/storage failure during resolution.
    #[error("identity resolution failed: {0}")]
    Internal(String),
}

impl From<notegate_core::Error> for IdentityError {
    fn from(error: notegate_core::Error) -> Self {
        match error {
            notegate_core::Error::Validation(_message) => Self::InvalidInput,
            error => Self::Internal(error.to_string()),
        }
    }
}

/// Resolves verified credentials into an authenticated [`Caller`].
#[derive(Debug, Clone)]
pub struct Resolver {
    users: AccountRepo,
    api_keys: ApiKeyRepo,
    crypto: PiiCrypto,
}

impl Resolver {
    pub fn new(users: AccountRepo, api_keys: ApiKeyRepo, crypto: PiiCrypto) -> Self {
        Self {
            users,
            api_keys,
            crypto,
        }
    }

    /// Resolve a browser login: create or update the user account, then return
    /// the caller on the browser channel. Inactive accounts remain rejected.
    pub async fn resolve_browser(&self, attrs: ResolveAttrs) -> Result<Caller, IdentityError> {
        let (account, user) = self.users.upsert_user_by_sub(&attrs).await?;
        caller_from_user(account, user, Channel::Browser)
    }

    /// Resolve a db-backed browser session by its owning user id.
    pub async fn resolve_browser_session_user(
        &self,
        user_id: Uuid,
    ) -> Result<Caller, IdentityError> {
        let resolved = self.users.find_caller_by_account_id(user_id).await?;
        let (account, user) = resolved.ok_or(IdentityError::NotRegistered)?;
        if account.kind != AccountKind::User {
            return Err(IdentityError::Inactive);
        }
        caller_from_user(account, user, Channel::Browser)
    }

    /// Resolve an MCP bearer for an already-registered user account.
    pub async fn resolve_mcp(&self, attrs: ResolveAttrs) -> Result<Caller, IdentityError> {
        self.resolve_registered_user(&attrs.sub, Channel::Mcp).await
    }

    /// Resolve a Command API OAuth bearer for an already-registered user account.
    pub async fn resolve_command_user(&self, attrs: ResolveAttrs) -> Result<Caller, IdentityError> {
        self.resolve_registered_user(&attrs.sub, Channel::Api).await
    }

    /// Resolve an Agent API key on the given channel.
    pub async fn resolve_agent_api_key(
        &self,
        token: &str,
        channel: Channel,
    ) -> Result<Caller, IdentityError> {
        let Some((key_id, secret, token_prefix)) = crate::api_keys::parse_token(token) else {
            return Err(IdentityError::NotRegistered);
        };
        let token_hash = self.crypto.api_key_hash(&key_id.to_string(), secret)?;
        let resolved = self
            .api_keys
            .find_live_agent_by_key(key_id, &token_prefix, &token_hash)
            .await?
            .ok_or(IdentityError::NotRegistered)?;
        let (account, agent) = resolved;
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
        caller_from_user(account, user, channel)
    }
}

/// Build a user caller, rejecting an inactive account.
pub fn caller_from_user(
    account: Account,
    user: User,
    channel: Channel,
) -> Result<Caller, IdentityError> {
    // A soft-deleted or deactivated account must never authenticate.
    if !account.is_live() {
        return Err(IdentityError::Inactive);
    }
    Ok(Caller {
        account,
        identity: CallerIdentity::User(user),
        channel,
    })
}
