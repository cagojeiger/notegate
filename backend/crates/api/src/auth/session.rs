use chrono::{DateTime, Utc};
use notegate_db::browser_session_repo::{
    InsertBrowserSession, LiveBrowserSessionCaller, format_token, parse_token, token_prefix,
};
use notegate_model::Caller;
use uuid::Uuid;

use crate::auth::bearer::{AuthError, map_identity_error};
use crate::state::AppState;

pub const BROWSER_SESSION_COOKIE: &str = "notegate_browser_session";

pub async fn create_browser_session(state: &AppState, user_id: Uuid) -> Result<String, AuthError> {
    let session_id = Uuid::new_v4();
    let secret = new_session_secret();
    let token_hash = state
        .security
        .browser_session_hash(&session_id.to_string(), &secret)
        .map_err(|_| AuthError::Internal)?;
    let now = Utc::now();
    let expires_at = checked_add(now, state.config.browser_session_max_ttl)?;
    let validated_until = checked_add(now, std::time::Duration::from_secs(86_400))?.min(expires_at);
    let prefix = token_prefix(session_id);
    state
        .browser_sessions
        .insert_session(
            InsertBrowserSession {
                session_id,
                user_id,
                token_prefix: &prefix,
                token_hash: &token_hash,
                validated_until,
                expires_at,
            },
            state.oidc.issuer(),
        )
        .await
        .map_err(|_| AuthError::Internal)?;
    Ok(format_token(session_id, &secret))
}

pub async fn verify_browser_session(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    let parsed = parse_browser_session_token(state, token)?;
    let resolved = state
        .browser_sessions
        .find_live_caller_by_token(parsed.session_id, &parsed.token_hash, &state.security)
        .await
        .map_err(|_| AuthError::Internal)?
        .ok_or(AuthError::InvalidToken)?;
    if resolved.oidc_issuer.as_deref() != Some(state.oidc.issuer())
        || resolved.validated_until <= Utc::now()
    {
        return Err(AuthError::InvalidToken);
    }
    state
        .metadata_writes
        .record_browser_session(resolved.session_id);
    caller_from_live_session(resolved)
}

pub async fn revoke_browser_session_for_logout(
    state: &AppState,
    token: Option<&str>,
) -> Result<(), AuthError> {
    let Some(token) = token else {
        return Ok(());
    };
    let parsed = match parse_browser_session_token(state, token) {
        Ok(parsed) => parsed,
        Err(AuthError::InvalidToken) => return Ok(()),
        Err(error) => return Err(error),
    };
    if let Some(session) = state
        .browser_sessions
        .find_by_token(parsed.session_id, &parsed.token_hash)
        .await
        .map_err(|_| AuthError::Internal)?
    {
        state
            .browser_sessions
            .revoke_session_for_logout(session.id)
            .await
            .map_err(|_| AuthError::Internal)?;
    }
    Ok(())
}

fn caller_from_live_session(resolved: LiveBrowserSessionCaller) -> Result<Caller, AuthError> {
    notegate_service::identity::caller_from_user(
        resolved.account,
        resolved.user,
        notegate_model::Channel::Browser,
    )
    .map_err(map_identity_error)
}

fn parse_browser_session_token(
    state: &AppState,
    token: &str,
) -> Result<ParsedBrowserSessionToken, AuthError> {
    let (session_id, secret) = parse_token(token).ok_or(AuthError::InvalidToken)?;
    let token_hash = state
        .security
        .browser_session_hash(&session_id.to_string(), secret)
        .map_err(|_error| AuthError::Internal)?;
    Ok(ParsedBrowserSessionToken {
        session_id,
        token_hash,
    })
}

fn checked_add(
    now: DateTime<Utc>,
    duration: std::time::Duration,
) -> Result<DateTime<Utc>, AuthError> {
    let duration = chrono::Duration::from_std(duration).map_err(|_error| AuthError::Internal)?;
    now.checked_add_signed(duration).ok_or(AuthError::Internal)
}

fn new_session_secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

struct ParsedBrowserSessionToken {
    session_id: Uuid,
    token_hash: String,
}
