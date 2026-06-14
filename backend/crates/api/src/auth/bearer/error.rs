use axum::Json;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::auth::metadata::{
    challenge_header, protected_resource_metadata_url, scoped_challenge_header,
};
use crate::identity::IdentityError;
use crate::state::AppState;

/// Map an identity-resolution failure to an [`AuthError`]. Shared by the bearer
/// and browser-session paths, which treat an unregistered subject the same way.
/// Agent-key auth maps `NotRegistered` differently and keeps its own variant.
pub fn map_identity_error(error: IdentityError) -> AuthError {
    match error {
        IdentityError::NotRegistered => AuthError::NotRegistered,
        IdentityError::Inactive => AuthError::Inactive,
        IdentityError::InvalidInput => AuthError::InvalidToken,
        IdentityError::Internal(_message) => AuthError::Internal,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing or malformed bearer token")]
    MissingToken,
    #[error("invalid token")]
    InvalidToken,
    #[error("user not registered")]
    NotRegistered,
    #[error("user is inactive")]
    Inactive,
    #[error("upstream/internal failure")]
    Internal,
}

pub fn auth_error_body(state: &AppState, error: &AuthError) -> serde_json::Value {
    let code = code_for_error(error);
    match error {
        AuthError::NotRegistered => serde_json::json!({
            "error": code,
            "kind": code,
            "message": message_for_error(error),
            "login_url": login_url(state),
            "mcp_url": mcp_url(state),
        }),
        _ => serde_json::json!({
            "error": code,
            "kind": code,
            "message": message_for_error(error),
        }),
    }
}

pub fn auth_error_response(state: &AppState, error: AuthError) -> Response {
    let status = status_for_error(&error);
    let code = code_for_error(&error);
    log_auth_denied(code, status, &error);
    let body = Json(auth_error_body(state, &error));
    let mut response = (status, body).into_response();
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            axum::http::header::WWW_AUTHENTICATE,
            shared_challenge_header(&state.config.resource_url),
        );
    }
    response
}

pub fn shared_challenge_header(resource_url: &str) -> HeaderValue {
    let meta = protected_resource_metadata_url(resource_url);
    challenge_header(&meta.full_url)
}

pub fn shared_scoped_challenge_header(resource_url: &str) -> HeaderValue {
    let meta = protected_resource_metadata_url(resource_url);
    scoped_challenge_header(&meta.full_url)
}

pub fn status_for_error(error: &AuthError) -> StatusCode {
    match error {
        AuthError::MissingToken | AuthError::InvalidToken => StatusCode::UNAUTHORIZED,
        AuthError::NotRegistered | AuthError::Inactive => StatusCode::FORBIDDEN,
        AuthError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn log_auth_denied(code: &'static str, status: StatusCode, error: &AuthError) {
    let status = status.as_u16();
    match error {
        AuthError::MissingToken => tracing::debug!(event = "auth.denied", error = code, status),
        AuthError::Internal => tracing::error!(event = "auth.denied", error = code, status),
        AuthError::InvalidToken | AuthError::NotRegistered | AuthError::Inactive => {
            tracing::warn!(event = "auth.denied", error = code, status);
        }
    }
}

fn login_url(state: &AppState) -> String {
    format!("{}/auth/login", state.config.notegate_public_url)
}

fn mcp_url(state: &AppState) -> String {
    state.config.resource_url.clone()
}

fn code_for_error(error: &AuthError) -> &'static str {
    match error {
        AuthError::MissingToken => "missing_token",
        AuthError::InvalidToken => "invalid_token",
        AuthError::NotRegistered => "not_registered",
        AuthError::Inactive => "inactive_account",
        AuthError::Internal => "internal_error",
    }
}

fn message_for_error(error: &AuthError) -> &'static str {
    match error {
        AuthError::MissingToken => "missing or malformed bearer token",
        AuthError::InvalidToken => "invalid token",
        AuthError::NotRegistered => {
            "This authgate account is authenticated but not registered in notegate yet. Open login_url once, then reconnect your MCP client."
        }
        AuthError::Inactive => "inactive account",
        AuthError::Internal => "internal server error",
    }
}
