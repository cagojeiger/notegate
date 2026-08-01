use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use notegate_model::Caller;

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
    let browser_session = match browser_session_token(request.headers()) {
        Ok(browser_session) => browser_session,
        Err(error) => return auth_error_response(&state, error, None),
    };
    if browser_session.is_some()
        && is_unsafe_method(request.method())
        && !has_trusted_browser_origin(request.headers(), &state)
    {
        return ApiError::forbidden(
            "browser session mutation requires same-origin Origin or Referer",
        )
        .into_response();
    }

    let caller = match authenticate_browser_session(&state, browser_session).await {
        Ok(caller) => caller,
        Err(error) => return auth_error_response(&state, error, None),
    };

    request.extensions_mut().insert(caller);
    next.run(request).await
}

pub async fn require_browser_session_for_docs(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let browser_session = browser_session_token(request.headers());
    let caller = match browser_session {
        Ok(browser_session) => authenticate_browser_session(&state, browser_session).await,
        Err(error) => Err(error),
    };
    let caller = match caller {
        Ok(caller) => caller,
        Err(AuthError::MissingToken | AuthError::InvalidToken) => {
            return redirect_to_login(request.uri());
        }
        Err(error) => return auth_error_response(&state, error, None),
    };

    request.extensions_mut().insert(caller);
    next.run(request).await
}

fn browser_session_token(headers: &HeaderMap) -> Result<Option<String>, AuthError> {
    if extract_bearer(headers).is_some() {
        return Err(AuthError::InvalidToken);
    }

    Ok(extract_cookie_value(headers, BROWSER_SESSION_COOKIE))
}

async fn authenticate_browser_session(
    state: &AppState,
    browser_session: Option<String>,
) -> Result<Caller, AuthError> {
    match browser_session {
        Some(session) => verify_browser_session(state, &session).await,
        None => Err(AuthError::MissingToken),
    }
}

fn redirect_to_login(uri: &Uri) -> Response {
    let next = uri
        .path_and_query()
        .map_or(uri.path(), axum::http::uri::PathAndQuery::as_str);
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("next", next);
    let query = query.finish();
    Redirect::temporary(&format!("/auth/login?{query}")).into_response()
}

fn is_unsafe_method(method: &Method) -> bool {
    !matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS)
}
