use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use notegate_model::Channel;

use crate::auth::api_key::verify_api_key;
use crate::auth::bearer::{
    AuthError, auth_error_response, extract_bearer, extract_cookie_value, verify_bearer,
};
use crate::auth::session::{BROWSER_SESSION_COOKIE, verify_browser_session};
use crate::state::AppState;

pub async fn require_bearer(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let bearer_token = extract_bearer(request.headers()).map(str::to_owned);
    let browser_session = extract_cookie_value(request.headers(), BROWSER_SESSION_COOKIE);

    let caller = match verify_request_caller(&state, bearer_token, browser_session).await {
        Ok(caller) => caller,
        Err(error) => return auth_error_response(&state, error),
    };

    request.extensions_mut().insert(caller);
    next.run(request).await
}

/// REST auth chain: bearer JWT → user, then the same bearer as an agent key →
/// agent, then browser cookie → user.
async fn verify_request_caller(
    state: &AppState,
    bearer_token: Option<String>,
    browser_session: Option<String>,
) -> Result<notegate_model::Caller, AuthError> {
    if let Some(token) = bearer_token {
        return match verify_bearer(state, &token).await {
            Ok(caller) => Ok(caller),
            // A bearer that is not a valid JWT may still be an agent key.
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
