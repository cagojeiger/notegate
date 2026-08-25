use notegate_model::Caller;

use crate::auth::bearer::{AuthError, map_identity_error};
use crate::state::AppState;

pub async fn verify_bearer_mcp(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    let resource = state.config.resource_url.trim_end_matches('/');
    let resource_with_slash = format!("{resource}/");
    let attrs = state
        .jwt
        .verify_for_audiences(token, &[resource, &resource_with_slash])
        .await?;
    state
        .resolver
        .resolve_mcp(attrs)
        .await
        .map_err(map_identity_error)
}

pub async fn verify_bearer_command(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    let attrs = state
        .jwt
        .verify_for_audiences(token, &[&state.config.cli_oauth_client_id])
        .await?;
    state
        .resolver
        .resolve_command_user(attrs)
        .await
        .map_err(map_identity_error)
}
