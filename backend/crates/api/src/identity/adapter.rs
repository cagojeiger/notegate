//! The request-time caller resolver seam.
//!
//! The api holds the resolver behind `Arc<dyn CallerResolver>` so `AppState`
//! stays object-safe. The concrete resolver is the `notegate-service`
//! [`Resolver`], implemented here for the api trait. `IdentityError` and
//! `ResolveAttrs` are re-exported by the service for the auth layer.

use std::future::Future;
use std::pin::Pin;

use notegate_model::{Caller, Channel};
use notegate_service::identity::Resolver;
use uuid::Uuid;

pub use notegate_service::identity::{IdentityError, ResolveAttrs};

/// Resolves verified credentials into an authenticated [`Caller`]. Object-safe
/// so `AppState` can hold it behind `Arc<dyn CallerResolver>`.
pub trait CallerResolver: Send + Sync {
    fn resolve_browser(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>>;

    fn resolve_browser_session_user(
        &self,
        user_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>>;

    fn resolve_mcp(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>>;

    fn resolve_command_user(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>>;

    /// Resolve an Agent API key (the raw plaintext token).
    fn resolve_agent_api_key(
        &self,
        token: String,
        channel: Channel,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>>;
}

impl CallerResolver for Resolver {
    fn resolve_browser(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve_browser(attrs).await })
    }

    fn resolve_browser_session_user(
        &self,
        user_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve_browser_session_user(user_id).await })
    }

    fn resolve_mcp(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve_mcp(attrs).await })
    }

    fn resolve_command_user(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve_command_user(attrs).await })
    }

    fn resolve_agent_api_key(
        &self,
        token: String,
        channel: Channel,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve_agent_api_key(&token, channel).await })
    }
}
