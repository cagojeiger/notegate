use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth::bearer::{AuthError, auth_error_response, extract_bearer, extract_cookie_value};
use crate::auth::origin::has_trusted_browser_origin;
use crate::auth::session::{BROWSER_SESSION_COOKIE, verify_browser_session};
use crate::error::ApiError;
use crate::state::AppState;

pub async fn require_browser_session(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    if extract_bearer(request.headers()).is_some() {
        return auth_error_response(&state, AuthError::InvalidToken, None);
    }

    let browser_session = extract_cookie_value(request.headers(), BROWSER_SESSION_COOKIE);
    if browser_session.is_some()
        && is_unsafe_method(request.method())
        && !has_trusted_browser_origin(request.headers(), &state)
    {
        return ApiError::forbidden(
            "browser session mutation requires same-origin Origin or Referer",
        )
        .into_response();
    }

    let caller = match browser_session {
        Some(session) => verify_browser_session(&state, &session).await,
        None => Err(AuthError::MissingToken),
    };
    let caller = match caller {
        Ok(caller) => caller,
        Err(error) => return auth_error_response(&state, error, None),
    };

    request.extensions_mut().insert(caller);
    next.run(request).await
}

fn is_unsafe_method(method: &Method) -> bool {
    !matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS)
}
