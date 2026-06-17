use chrono::{DateTime, Utc};
use notegate_core::security::EncryptedField;
use notegate_db::browser_session_repo::{
    BrowserSession, InsertBrowserSession, RotatedRefreshToken, format_token, parse_token,
    token_prefix,
};
use notegate_model::Caller;
use uuid::Uuid;

use crate::auth::bearer::{AuthError, map_identity_error};
use crate::auth::oauth_exchange::{RefreshUserInfoError, RefreshedUserInfo, refresh_userinfo};
use crate::state::AppState;

pub const BROWSER_SESSION_COOKIE: &str = "notegate_browser_session";
const REFRESH_FAILED_REASON: &str = "refresh_failed";
const LOGOUT_REASON: &str = "logout";

pub async fn create_browser_session(
    state: &AppState,
    user_id: Uuid,
    refresh_token: &str,
) -> Result<String, AuthError> {
    let session_id = Uuid::new_v4();
    let secret = new_session_secret();
    let token_hash = state
        .security
        .browser_session_hash(&session_id.to_string(), &secret)
        .map_err(|_error| AuthError::Internal)?;
    let encrypted_refresh_token = encrypt_refresh_token(state, session_id, refresh_token)?;
    let now = Utc::now();
    let validated_until = checked_add(now, state.config.browser_session_ttl)?;
    let expires_at = checked_add(now, state.config.browser_session_max_ttl)?;
    let token_prefix = token_prefix(session_id);
    state
        .browser_sessions
        .insert_session(InsertBrowserSession {
            session_id,
            user_id,
            token_prefix: &token_prefix,
            token_hash: &token_hash,
            refresh_token: &encrypted_refresh_token,
            refresh_token_enc_key_id: state.security.enc_key_id(),
            refresh_token_enc_version: state.security.version(),
            validated_until,
            expires_at,
        })
        .await
        .map_err(|_error| AuthError::Internal)?;
    Ok(format_token(session_id, &secret))
}

pub async fn verify_browser_session(state: &AppState, token: &str) -> Result<Caller, AuthError> {
    let parsed = parse_browser_session_token(state, token)?;
    let session = state
        .browser_sessions
        .find_live_by_token(parsed.session_id, &parsed.token_hash)
        .await
        .map_err(|_error| AuthError::Internal)?
        .ok_or(AuthError::InvalidToken)?;

    if session.validated_until > Utc::now() {
        state
            .browser_sessions
            .touch_last_used(session.id)
            .await
            .map_err(|_error| AuthError::Internal)?;
        return resolve_browser_session_user(state, session.user_id).await;
    }

    refresh_browser_session(state, parsed).await
}

pub async fn revoke_browser_session_for_logout(
    state: &AppState,
    token: Option<&str>,
) -> Option<String> {
    let token = token?;
    let parsed = parse_browser_session_token(state, token).ok()?;
    let session = state
        .browser_sessions
        .find_by_token(parsed.session_id, &parsed.token_hash)
        .await
        .ok()
        .flatten()?;
    let refresh_token = decrypt_refresh_token(state, &session).ok();
    if let Err(error) = state
        .browser_sessions
        .revoke_session(session.id, LOGOUT_REASON)
        .await
    {
        tracing::warn!(event = "browser_session.logout_revoke_failed", %error);
    }
    refresh_token
}

async fn refresh_browser_session(
    state: &AppState,
    parsed: ParsedBrowserSessionToken,
) -> Result<Caller, AuthError> {
    let refresh_claim_id = Uuid::new_v4();
    let session = match claim_refresh_session(state, &parsed, refresh_claim_id).await? {
        Some(session) => session,
        None => return resolve_after_refresh_claim_miss(state, parsed).await,
    };
    let refresh_token = decrypt_claimed_refresh_token(state, &session, refresh_claim_id).await?;
    let refreshed =
        refresh_claimed_userinfo(state, &session, refresh_claim_id, &refresh_token).await?;
    ensure_refreshed_sub_matches_session(state, &session, refresh_claim_id, &refreshed).await?;
    persist_successful_refresh(state, &session, refresh_claim_id, &refreshed).await?;

    resolve_browser_session_user(state, session.user_id).await
}

async fn claim_refresh_session(
    state: &AppState,
    parsed: &ParsedBrowserSessionToken,
    refresh_claim_id: Uuid,
) -> Result<Option<BrowserSession>, AuthError> {
    state
        .browser_sessions
        .claim_refresh(parsed.session_id, &parsed.token_hash, refresh_claim_id)
        .await
        .map_err(|_error| AuthError::Internal)
}

async fn decrypt_claimed_refresh_token(
    state: &AppState,
    session: &BrowserSession,
    refresh_claim_id: Uuid,
) -> Result<String, AuthError> {
    let refresh_token = match decrypt_refresh_token(state, session) {
        Ok(token) => token,
        Err(error) => {
            clear_refresh_claim(state, session.id, refresh_claim_id).await?;
            return Err(error);
        }
    };
    Ok(refresh_token)
}

async fn refresh_claimed_userinfo(
    state: &AppState,
    session: &BrowserSession,
    refresh_claim_id: Uuid,
    refresh_token: &str,
) -> Result<RefreshedUserInfo, AuthError> {
    let refreshed = match refresh_userinfo(&state.oidc, &state.http, refresh_token).await {
        Ok(refreshed) => refreshed,
        Err(RefreshUserInfoError::InvalidGrant(error)) => {
            tracing::warn!(event = "browser_session.refresh_rejected", %error);
            state
                .browser_sessions
                .revoke_claimed_refresh(session.id, refresh_claim_id, REFRESH_FAILED_REASON)
                .await
                .map_err(|_error| AuthError::Internal)?;
            return Err(AuthError::InvalidToken);
        }
        Err(RefreshUserInfoError::Transient {
            message,
            rotated_refresh_token,
        }) => {
            tracing::warn!(event = "browser_session.refresh_unavailable", error = %message);
            clear_refresh_claim_after_transient(
                state,
                session.id,
                refresh_claim_id,
                rotated_refresh_token.as_deref(),
            )
            .await?;
            return Err(AuthError::Unavailable);
        }
    };
    Ok(refreshed)
}

async fn ensure_refreshed_sub_matches_session(
    state: &AppState,
    session: &BrowserSession,
    refresh_claim_id: Uuid,
    refreshed: &RefreshedUserInfo,
) -> Result<(), AuthError> {
    let same_user = match refreshed_sub_matches_session(
        state,
        session.user_id,
        &refreshed.userinfo.sub,
    )
    .await
    {
        Ok(same_user) => same_user,
        Err(error) => {
            clear_refresh_claim(state, session.id, refresh_claim_id).await?;
            return Err(error);
        }
    };
    if !same_user {
        tracing::warn!(event = "browser_session.refresh_sub_mismatch");
        state
            .browser_sessions
            .revoke_claimed_refresh(session.id, refresh_claim_id, REFRESH_FAILED_REASON)
            .await
            .map_err(|_error| AuthError::Internal)?;
        return Err(AuthError::InvalidToken);
    }
    Ok(())
}

async fn persist_successful_refresh(
    state: &AppState,
    session: &BrowserSession,
    refresh_claim_id: Uuid,
    refreshed: &RefreshedUserInfo,
) -> Result<(), AuthError> {
    let rotated = match encrypted_rotated_refresh_token(
        state,
        session.id,
        refreshed.refresh_token.as_deref(),
    ) {
        Ok(rotated) => rotated,
        Err(error) => {
            clear_refresh_claim(state, session.id, refresh_claim_id).await?;
            return Err(error);
        }
    };
    let rotated = rotated.as_ref().map(|refresh_token| RotatedRefreshToken {
        refresh_token,
        refresh_token_enc_key_id: state.security.enc_key_id(),
        refresh_token_enc_version: state.security.version(),
    });
    let validated_until = match next_validated_until(state, session) {
        Ok(validated_until) => validated_until,
        Err(error) => {
            clear_refresh_claim(state, session.id, refresh_claim_id).await?;
            return Err(error);
        }
    };
    let refreshed = state
        .browser_sessions
        .mark_refreshed(session.id, refresh_claim_id, validated_until, rotated)
        .await
        .map_err(|_error| AuthError::Internal)?;
    if !refreshed {
        return Err(AuthError::InvalidToken);
    }

    Ok(())
}

fn encrypted_rotated_refresh_token(
    state: &AppState,
    session_id: Uuid,
    refresh_token: Option<&str>,
) -> Result<Option<EncryptedField>, AuthError> {
    refresh_token
        .map(|token| encrypt_refresh_token(state, session_id, token))
        .transpose()
}

fn next_validated_until(
    state: &AppState,
    session: &BrowserSession,
) -> Result<DateTime<Utc>, AuthError> {
    Ok(checked_add(Utc::now(), state.config.browser_session_ttl)?.min(session.expires_at))
}

async fn resolve_after_refresh_claim_miss(
    state: &AppState,
    parsed: ParsedBrowserSessionToken,
) -> Result<Caller, AuthError> {
    let session = state
        .browser_sessions
        .find_live_by_token(parsed.session_id, &parsed.token_hash)
        .await
        .map_err(|_error| AuthError::Internal)?
        .ok_or(AuthError::InvalidToken)?;
    if session.validated_until > Utc::now() {
        state
            .browser_sessions
            .touch_last_used(session.id)
            .await
            .map_err(|_error| AuthError::Internal)?;
        return resolve_browser_session_user(state, session.user_id).await;
    }
    Err(AuthError::Unavailable)
}

async fn clear_refresh_claim(
    state: &AppState,
    session_id: Uuid,
    refresh_claim_id: Uuid,
) -> Result<(), AuthError> {
    state
        .browser_sessions
        .clear_refresh_claim(session_id, refresh_claim_id)
        .await
        .map_err(|_error| AuthError::Internal)?;
    Ok(())
}

async fn clear_refresh_claim_after_transient(
    state: &AppState,
    session_id: Uuid,
    refresh_claim_id: Uuid,
    rotated_refresh_token: Option<&str>,
) -> Result<(), AuthError> {
    let Some(rotated_refresh_token) = rotated_refresh_token else {
        return clear_refresh_claim(state, session_id, refresh_claim_id).await;
    };
    let encrypted_refresh_token =
        match encrypt_refresh_token(state, session_id, rotated_refresh_token) {
            Ok(encrypted_refresh_token) => encrypted_refresh_token,
            Err(error) => {
                clear_refresh_claim(state, session_id, refresh_claim_id).await?;
                return Err(error);
            }
        };
    state
        .browser_sessions
        .store_rotated_refresh_token_and_clear_claim(
            session_id,
            refresh_claim_id,
            RotatedRefreshToken {
                refresh_token: &encrypted_refresh_token,
                refresh_token_enc_key_id: state.security.enc_key_id(),
                refresh_token_enc_version: state.security.version(),
            },
        )
        .await
        .map_err(|_error| AuthError::Internal)?;
    Ok(())
}

async fn refreshed_sub_matches_session(
    state: &AppState,
    user_id: Uuid,
    sub: &str,
) -> Result<bool, AuthError> {
    let resolved = state
        .accounts
        .find_user_by_sub(sub)
        .await
        .map_err(|_error| AuthError::Internal)?;
    Ok(resolved.is_some_and(|(account, _user)| account.id == user_id))
}

async fn resolve_browser_session_user(
    state: &AppState,
    user_id: Uuid,
) -> Result<Caller, AuthError> {
    state
        .resolver
        .resolve_browser_session_user(user_id)
        .await
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

fn encrypt_refresh_token(
    state: &AppState,
    session_id: Uuid,
    refresh_token: &str,
) -> Result<EncryptedField, AuthError> {
    state
        .security
        .encrypt_browser_refresh_token(&session_id.to_string(), refresh_token)
        .map_err(|_error| AuthError::Internal)
}

fn decrypt_refresh_token(state: &AppState, session: &BrowserSession) -> Result<String, AuthError> {
    state
        .security
        .decrypt_browser_refresh_token(
            &session.id.to_string(),
            &session.refresh_token_enc_key_id,
            session.refresh_token_enc_version,
            &session.refresh_token,
        )
        .map_err(|_error| AuthError::Internal)
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
