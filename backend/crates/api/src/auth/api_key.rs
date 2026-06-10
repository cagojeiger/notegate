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
        // For API-key auth, both an unrecognized key and an inactive account map to
        // an invalid token (401), never revealing whether the credential exists or
        // was deactivated. (docs/spec/rest/errors.md, docs/spec/mcp/auth.md). The
        // live-key lookup already pre-filters inactive accounts, so `Inactive` is not
        // normally reachable here; this keeps the mapping spec-correct regardless.
        IdentityError::NotRegistered | IdentityError::Inactive => AuthError::InvalidToken,
        IdentityError::InvalidInput => AuthError::InvalidToken,
        IdentityError::Internal(_message) => AuthError::Internal,
    }
}
