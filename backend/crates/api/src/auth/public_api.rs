use axum::body::Body;
use axum::extract::State;
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;
use notegate_model::Channel;

use crate::auth::api_key::verify_agent_api_key;
use crate::auth::bearer::{AuthError, auth_error_response, extract_bearer};
use crate::state::AppState;

pub async fn require_public_api_key(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    require_agent_api_key(
        &state,
        request,
        next,
        HeaderValue::from_static("Bearer realm=\"notegate-public-api\""),
    )
    .await
}

pub async fn require_command_api_key(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    require_agent_api_key(
        &state,
        request,
        next,
        HeaderValue::from_static("Bearer realm=\"notegate-command-api\""),
    )
    .await
}

async fn require_agent_api_key(
    state: &AppState,
    mut request: Request<Body>,
    next: Next,
    challenge: HeaderValue,
) -> Response {
    let caller = match extract_bearer(request.headers()) {
        Some(token) => verify_agent_api_key(state, token, Channel::Api).await,
        None => Err(AuthError::MissingToken),
    };
    let caller = match caller {
        Ok(caller) => caller,
        Err(error) => {
            return auth_error_response(state, error, Some(challenge));
        }
    };

    request.extensions_mut().insert(caller);
    next.run(request).await
}

pub async fn mark_private_no_store(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    set_private_no_store(&mut response);
    response
}

pub fn set_private_no_store(response: &mut Response) {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
}
