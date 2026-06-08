//! The request-time caller resolver seam.
//!
//! The api holds the resolver behind `Arc<dyn CallerResolver>` so `AppState`
//! stays object-safe. The concrete resolver is the `notegate-service`
//! [`Resolver`], implemented here for the api trait. `IdentityError` and
//! `ResolveAttrs` are owned by the service and re-exported for the auth layer.

use std::future::Future;
use std::pin::Pin;

use notegate_model::{Caller, Channel};
use notegate_service::identity::{AgentAuthStore, Resolver, UserStore};

pub use notegate_service::identity::{IdentityError, ResolveAttrs};

/// Resolves verified credentials into an authenticated [`Caller`]. Object-safe
/// so `AppState` can hold it behind `Arc<dyn CallerResolver>`.
pub trait CallerResolver: Send + Sync {
    fn resolve_browser(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>>;

    fn resolve_browser_session(
        &self,
        sub: String,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>>;

    fn resolve_api(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>>;

    fn resolve_mcp(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>>;

    /// Resolve an agent key (the raw plaintext token) into an agent caller.
    fn resolve_api_key(
        &self,
        token: String,
        channel: Channel,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>>;
}

impl<U, A> CallerResolver for Resolver<U, A>
where
    U: UserStore,
    A: AgentAuthStore,
{
    fn resolve_browser(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve_browser(attrs).await })
    }

    fn resolve_browser_session(
        &self,
        sub: String,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve_browser_session(&sub).await })
    }

    fn resolve_api(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve_api(attrs).await })
    }

    fn resolve_mcp(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve_mcp(attrs).await })
    }

    fn resolve_api_key(
        &self,
        token: String,
        channel: Channel,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve_api_key(&token, channel).await })
    }
}
