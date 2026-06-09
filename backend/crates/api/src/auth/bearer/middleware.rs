use axum::body::Body;
use axum::extract::State;
use axum::http::header::{ORIGIN, REFERER};
use axum::http::{HeaderMap, Method, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use notegate_model::Channel;

use crate::auth::api_key::verify_api_key;
use crate::auth::bearer::{
    AuthError, auth_error_response, extract_bearer, extract_cookie_value, verify_bearer,
};
use crate::auth::session::{BROWSER_SESSION_COOKIE, verify_browser_session};
use crate::error::ApiError;
use crate::state::AppState;

pub async fn require_bearer(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let bearer_token = extract_bearer(request.headers()).map(str::to_owned);
    let browser_session = extract_cookie_value(request.headers(), BROWSER_SESSION_COOKIE);

    if bearer_token.is_none()
        && browser_session.is_some()
        && is_unsafe_method(request.method())
        && !has_trusted_browser_origin(request.headers(), &state)
    {
        return ApiError::forbidden(
            "browser session mutation requires same-origin Origin or Referer",
        )
        .into_response();
    }

    let caller = match verify_request_caller(&state, bearer_token, browser_session).await {
        Ok(caller) => caller,
        Err(error) => return auth_error_response(&state, error),
    };

    request.extensions_mut().insert(caller);
    next.run(request).await
}

/// REST auth chain: bearer JWT → user, then the same bearer as a notegate API
/// key → user/agent, then browser cookie → user.
async fn verify_request_caller(
    state: &AppState,
    bearer_token: Option<String>,
    browser_session: Option<String>,
) -> Result<notegate_model::Caller, AuthError> {
    if let Some(token) = bearer_token {
        return match verify_bearer(state, &token).await {
            Ok(caller) => Ok(caller),
            // A bearer that is not a valid JWT may still be a notegate API key.
            Err(AuthError::InvalidToken | AuthError::MissingToken) => {
                verify_api_key(state, &token, Channel::Api).await
            }
            // A valid JWT whose account is missing/inactive is terminal.
            Err(error) => Err(error),
        };
    }

    if let Some(session) = browser_session {
        return verify_browser_session(state, &session).await;
    }

    Err(AuthError::MissingToken)
}

fn is_unsafe_method(method: &Method) -> bool {
    !matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS)
}

fn has_trusted_browser_origin(headers: &HeaderMap, state: &AppState) -> bool {
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .or_else(|| headers.get(REFERER).and_then(|value| value.to_str().ok()))
        .is_some_and(|source| {
            same_origin(source, &state.config.notegate_public_url)
                || same_origin(source, &state.config.resource_url)
        })
}

fn same_origin(source: &str, trusted: &str) -> bool {
    let Ok(source) = url::Url::parse(source) else {
        return false;
    };
    let Ok(trusted) = url::Url::parse(trusted) else {
        return false;
    };

    source.scheme() == trusted.scheme()
        && source.host_str() == trusted.host_str()
        && source.port_or_known_default() == trusted.port_or_known_default()
}
