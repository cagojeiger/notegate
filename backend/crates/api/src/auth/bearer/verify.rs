use notegate_model::Caller;

use crate::auth::bearer::{AuthError, map_identity_error};
use crate::state::AppState;

pub async fn verify_bearer_mcp(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    verify_local(
        state,
        token,
        &state.config.resource_url,
        "mcp",
        notegate_model::Channel::Mcp,
    )
    .await
}

pub async fn verify_bearer_command(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    verify_local(
        state,
        token,
        &format!("{}/cli", state.config.notegate_public_url),
        "cli",
        notegate_model::Channel::Api,
    )
    .await
}

async fn verify_local(
    state: &AppState,
    token: &str,
    resource: &str,
    scope: &str,
    channel: notegate_model::Channel,
) -> Result<Caller, AuthError> {
    // NoteGate issues 32-byte base64url secrets (43 characters). Do not query
    // OAuth storage for JWTs, Agent keys, or browser session credentials.
    if token.len() != 43 {
        return Err(AuthError::InvalidToken);
    }
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
