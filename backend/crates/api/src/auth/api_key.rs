//! Shared API-key verification for transports that already extracted a raw
//! credential and selected a [`Channel`].
//!
//! Hashing and account resolution live in `notegate-service`; SQL lives in
//! `notegate-db`. The token plaintext is never logged.

use notegate_model::{Caller, Channel};
use notegate_service::api_keys::parse_token_id;

use crate::auth::bearer::AuthError;
use crate::identity::IdentityError;
use crate::state::AppState;

/// Resolve a raw bearer token as an Agent API key on the given channel.
pub async fn verify_agent_api_key(
    state: &AppState,
    token: &str,
    channel: Channel,
) -> Result<Caller, AuthError> {
    let caller = state
        .resolver
        .resolve_agent_api_key(token.to_owned(), channel)
        .await
        .map_err(map_api_key_identity_error)?;
    // A successful production resolution implies a well-formed token. Keep the
    // parse optional so test resolvers can use opaque credentials.
    if let Some(key_id) = parse_token_id(token) {
        state.metadata_writes.record_api_key(key_id);
    }
    Ok(caller)
}

fn map_api_key_identity_error(error: IdentityError) -> AuthError {
    match error {
        // Both an unknown key and an inactive account map to 401, never revealing
        // whether the credential exists or was deactivated.
        IdentityError::NotRegistered | IdentityError::Inactive => AuthError::InvalidToken,
        IdentityError::InvalidInput => AuthError::InvalidToken,
        IdentityError::Internal(_message) => AuthError::Internal,
    }
}
