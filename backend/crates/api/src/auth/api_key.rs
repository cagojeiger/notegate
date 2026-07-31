//! Shared API-key verification for transports that already extracted a raw
//! credential and selected a [`Channel`].
//!
//! Hashing and account resolution live in `notegate-service`; SQL lives in
//! `notegate-db`. The token plaintext is never logged.

use notegate_model::{Caller, Channel};

use crate::auth::bearer::AuthError;
use crate::identity::IdentityError;
use crate::state::AppState;

/// Resolve a raw bearer token as an API key on the given channel.
pub async fn verify_api_key(
    state: &AppState,
    token: &str,
    channel: Channel,
) -> Result<Caller, AuthError> {
    state
        .resolver
        .resolve_api_key(token.to_owned(), channel)
        .await
        .map_err(map_api_key_identity_error)
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
