use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use notegate_db::browser_session_repo::{
    InsertBrowserSession, LiveBrowserSessionCaller, format_token, parse_token, token_prefix,
};
use notegate_model::Caller;
use uuid::Uuid;

use crate::auth::bearer::{AuthError, auth_error_response, map_identity_error};
use crate::auth::origin::has_trusted_browser_origin;
use crate::error::ApiError;
use crate::state::AppState;

pub const BROWSER_SESSION_COOKIE: &str = "notegate_browser_session";

pub async fn create_browser_session(
    state: &AppState,
    user_id: Uuid,
    user_agent: Option<&str>,
) -> Result<String, AuthError> {
    let session_id = Uuid::new_v4();
    let secret = new_session_secret();
    let token_hash = state
        .security
        .browser_session_hash(&session_id.to_string(), &secret)
        .map_err(|_| AuthError::Internal)?;
    let now = Utc::now();
    let expires_at = checked_add(now, state.config.browser_session_max_ttl)?;
    let idle_expires_at = now
        .checked_add_signed(chrono::Duration::days(i64::from(
            notegate_core::limits::BROWSER_SESSION_IDLE_TTL_DAYS,
        )))
        .ok_or(AuthError::Internal)?
        .min(expires_at);
    let prefix = token_prefix(session_id);
    let environment = client_environment(user_agent);
    state
        .browser_sessions
        .insert_session(
            InsertBrowserSession {
                session_id,
                user_id,
                token_prefix: &prefix,
                token_hash: &token_hash,
                client_environment: environment.as_deref(),
                idle_expires_at,
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
    state
        .metadata_writes
        .record_browser_session(resolved.session_id);
    caller_from_live_session(resolved)
}

pub async fn activity(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Response {
    if !has_trusted_browser_origin(&headers, &state) {
        return ApiError::forbidden("session activity requires same-origin Origin or Referer")
            .into_response();
    }
    let result = async {
        if headers.contains_key(header::AUTHORIZATION) {
            return Err(AuthError::InvalidToken);
        }
        let token = jar
            .get(BROWSER_SESSION_COOKIE)
            .ok_or(AuthError::MissingToken)?;
        let parsed = parse_browser_session_token(&state, token.value())?;
        let renewed = state
            .browser_sessions
            .renew_idle_expiry(parsed.session_id, &parsed.token_hash)
            .await
            .map_err(|_| AuthError::Internal)?;
        if !renewed {
            return Err(AuthError::InvalidToken);
        }
        Ok(())
    }
    .await;
    let mut response = match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => auth_error_response(&state, error, None),
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response
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

pub(super) fn client_environment(user_agent: Option<&str>) -> Option<String> {
    let ua = user_agent.filter(|value| !value.is_empty() && value.len() <= 2048)?;
    let browser = if ua.starts_with("notegate-cli/") {
        "NoteGate CLI"
    } else if ua.contains("Edg/") || ua.contains("EdgiOS/") || ua.contains("EdgA/") {
        "Edge"
    } else if ua.contains("OPR/") || ua.contains("OPiOS/") {
        "Opera"
    } else if ua.contains("Firefox/") || ua.contains("FxiOS/") {
        "Firefox"
    } else if ua.contains("Chrome/") || ua.contains("CriOS/") {
        "Chrome"
    } else if ua.contains("Safari/") {
        "Safari"
    } else {
        "Unknown browser"
    };
    let os = if ua.starts_with("notegate-cli/") && ua.contains("(macos;") {
        "macOS"
    } else if ua.starts_with("notegate-cli/") && ua.contains("(linux;") {
        "Linux"
    } else if ua.starts_with("notegate-cli/") && ua.contains("(windows;") {
        "Windows"
    } else if ua.contains("Android") {
        "Android"
    } else if ua.contains("iPhone") || ua.contains("iPod") {
        "iOS"
    } else if ua.contains("iPad") || (ua.contains("Macintosh") && ua.contains("Mobile/")) {
        "iPadOS"
    } else if ua.contains("Windows") {
        "Windows"
    } else if ua.contains("Macintosh") || ua.contains("Mac OS X") {
        "macOS"
    } else if ua.contains("CrOS") {
        "ChromeOS"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        "Unknown OS"
    };
    Some(format!("{browser} on {os}"))
}

#[cfg(test)]
mod environment_tests {
    use super::client_environment;

    #[test]
    fn records_only_coarse_environment_labels() {
        for (ua, expected) in [
            ("Macintosh Chrome/130 Safari/537", "Chrome on macOS"),
            ("Windows Chrome/130 Safari/537 Edg/130", "Edge on Windows"),
            ("iPhone CriOS/130 Mobile/15 Safari/604", "Chrome on iOS"),
            ("Macintosh Mobile/15 Safari/604", "Safari on iPadOS"),
            ("Android Chrome/130", "Chrome on Android"),
            ("Linux Firefox/130", "Firefox on Linux"),
            (
                "notegate-cli/0.1.113 (macos; aarch64)",
                "NoteGate CLI on macOS",
            ),
            (
                "unrecognized-private-value",
                "Unknown browser on Unknown OS",
            ),
        ] {
            assert_eq!(client_environment(Some(ua)).as_deref(), Some(expected));
        }
        assert!(client_environment(None).is_none());
        assert!(client_environment(Some(&"x".repeat(2049))).is_none());
    }
}
