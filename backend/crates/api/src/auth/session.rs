use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use notegate_domain::{Caller, IdentityError, ResolveAttrs};
use serde::{Deserialize, Serialize};

use crate::auth::bearer::AuthError;
use crate::state::AppState;

pub const BROWSER_SESSION_COOKIE: &str = "notegate_browser_session";

#[derive(Debug, Serialize, Deserialize)]
struct BrowserSessionClaims {
    sub: String,
    exp: usize,
}

pub fn create_browser_session(state: &AppState, sub: &str) -> Result<String, AuthError> {
    let now = chrono::Utc::now().timestamp();
    let ttl = i64::try_from(state.config.browser_session_ttl.as_secs())
        .map_err(|_error| AuthError::Internal)?;
    let exp = now.checked_add(ttl).ok_or(AuthError::Internal)? as usize;
    let claims = BrowserSessionClaims {
        sub: sub.to_owned(),
        exp,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.config.browser_session_secret.as_bytes()),
    )
    .map_err(|_error| AuthError::Internal)
}

pub async fn verify_browser_session(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    let data = decode::<BrowserSessionClaims>(
        token,
        &DecodingKey::from_secret(state.config.browser_session_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_error| AuthError::InvalidToken)?;

    state
        .resolver
        .resolve_api(ResolveAttrs {
            sub: data.claims.sub,
            email: String::new(),
            name: String::new(),
        })
        .await
        .map_err(map_identity_error)
}

fn map_identity_error(error: IdentityError) -> AuthError {
    match error {
        IdentityError::NotRegistered => AuthError::NotRegistered,
        IdentityError::Inactive => AuthError::Inactive,
        IdentityError::Store(_error) => AuthError::Internal,
    }
}
