//! API-key authentication: extract the raw bearer token and resolve it to a user
//! or agent [`Caller`] via the service resolver.
//!
//! This is the EXTRACTION half of API-key auth; the hashing/lookup logic lives
//! in `notegate-service` and the SQL in `notegate-db`. The token plaintext is
//! never logged.

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
        .map_err(map_identity_error)
}

fn map_identity_error(error: IdentityError) -> AuthError {
    match error {
        // An unrecognized API key is an invalid credential, not a "registered
        // user without an account" — surface it as an invalid token.
        IdentityError::NotRegistered => AuthError::InvalidToken,
        IdentityError::Inactive => AuthError::Inactive,
        IdentityError::Internal(_message) => AuthError::Internal,
    }
}
