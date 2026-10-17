use notegate_model::Caller;

use crate::auth::bearer::{AuthError, map_identity_error};
use crate::state::AppState;

pub async fn verify_bearer_mcp(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    if state.config.oidc.is_some() {
        return verify_local(
            state,
            token,
            &state.config.resource_url,
            "mcp",
            notegate_model::Channel::Mcp,
        )
        .await;
    }
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
    if state.config.oidc.is_some() {
        return verify_local(
            state,
            token,
            &format!("{}/cli", state.config.notegate_public_url),
            "cli",
            notegate_model::Channel::Api,
        )
        .await;
    }
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

async fn verify_local(
    state: &AppState,
    token: &str,
    resource: &str,
    scope: &str,
    channel: notegate_model::Channel,
) -> Result<Caller, AuthError> {
    let user = state
        .oauth
        .access_user(token, resource, scope, chrono::Utc::now())
        .await
        .map_err(|_| AuthError::Internal)?
        .ok_or(AuthError::InvalidToken)?;
    let mut caller = state
        .resolver
        .resolve_browser_session_user(user)
        .await
        .map_err(map_identity_error)?;
    caller.channel = channel;
    Ok(caller)
}
