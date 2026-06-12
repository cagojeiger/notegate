use notegate_model::Caller;

use crate::auth::bearer::{AuthError, map_identity_error};
use crate::identity::ResolveAttrs;
use crate::state::AppState;

pub async fn verify_bearer(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    let attrs = authenticate(state, token).await?;
    state
        .resolver
        .resolve_api(attrs)
        .await
        .map_err(map_identity_error)
}

pub async fn verify_bearer_mcp(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    let attrs = authenticate(state, token).await?;
    state
        .resolver
        .resolve_mcp(attrs)
        .await
        .map_err(map_identity_error)
}

/// Verify the bearer JWT against JWKS and extract identity attributes.
/// Shared by the REST and MCP paths, which differ only in how they resolve
/// the verified attributes into a `Caller`.
async fn authenticate(state: &AppState, token: &str) -> Result<ResolveAttrs, AuthError> {
    state.jwt.verify(token).await
}
