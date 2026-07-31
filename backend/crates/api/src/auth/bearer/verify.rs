use notegate_model::Caller;

use crate::auth::bearer::{AuthError, map_identity_error};
use crate::identity::ResolveAttrs;
use crate::state::AppState;

pub async fn verify_bearer_mcp(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    let attrs = authenticate(state, token).await?;
    state
        .resolver
        .resolve_mcp(attrs)
        .await
        .map_err(map_identity_error)
}

async fn authenticate(state: &AppState, token: &str) -> Result<ResolveAttrs, AuthError> {
    state.jwt.verify(token).await
}
