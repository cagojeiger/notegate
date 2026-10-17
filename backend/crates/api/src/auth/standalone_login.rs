//! Browser-bound, single-use server-side OIDC state. Each tab has its own cookie.
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::oauth_flow::{hardened_cookie, new_login_flow};
use super::page::html_page;
use crate::state::AppState;

#[derive(Serialize, Deserialize)]
struct LoginState {
    verifier: String,
    nonce: String,
    next: Option<String>,
}

pub(super) async fn login(state: &AppState, jar: CookieJar, next: Option<String>) -> Response {
    match prepare(state, next).await {
        Ok((id, secret, url)) => (
            jar.add(hardened_cookie(
                cookie_name(id),
                secret,
                300,
                state.config.secure_cookies,
            )),
            Redirect::temporary(&url),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(event="oidc.prepare_failed",%error);
            html_page(
                StatusCode::SERVICE_UNAVAILABLE,
                "Login unavailable",
                "Please try again later.",
            )
        }
    }
}

async fn prepare(
    state: &AppState,
    next: Option<String>,
) -> notegate_core::Result<(Uuid, String, String)> {
    let flow = new_login_flow(&state.oidc).await?;
    let id = Uuid::parse_str(&flow.csrf_state)
        .map_err(|_| notegate_core::Error::internal("invalid OIDC state"))?;
    let secret = notegate_service::oauth::random_secret();
    let payload = serde_json::to_string(&LoginState {
        verifier: flow.pkce_verifier,
        nonce: flow.nonce,
        next,
    })
    .map_err(|_| notegate_core::Error::internal("OIDC state serialization failed"))?;
    let encrypted = state
        .security
        .encrypt_oauth_state(&id.to_string(), &payload)?;
    state
        .oauth
        .store
        .insert_login_flow(
            id,
            &state.security.oauth_hash("login-cookie", &secret)?,
            state.oidc.issuer(),
            serde_json::to_value(encrypted)
                .map_err(|_| notegate_core::Error::internal("OIDC state serialization failed"))?,
            Utc::now() + Duration::minutes(5),
        )
        .await?;
    Ok((id, secret, flow.redirect_url))
}

pub(super) async fn callback(
    state: &AppState,
    jar: CookieJar,
    code: Option<String>,
    query_state: Option<String>,
    error: Option<String>,
) -> Response {
    let Some(id) = query_state.and_then(|s| Uuid::parse_str(&s).ok()) else {
        return invalid();
    };
    let name = cookie_name(id);
    let Some(cookie) = jar.get(&name) else {
        return invalid();
    };
    let hash = match state.security.oauth_hash("login-cookie", cookie.value()) {
        Ok(hash) => hash,
        Err(_) => return invalid(),
    };
    let jar = jar.add(hardened_cookie(
        name,
        String::new(),
        0,
        state.config.secure_cookies,
    ));
    let flow = match consume(state, id, &hash).await {
        Ok(Some(flow)) => flow,
        _ => return (jar, invalid()).into_response(),
    };
    let Some(code) = code.filter(|_| error.is_none()) else {
        return (jar, invalid()).into_response();
    };
    let user = match super::oauth_exchange::exchange_code_for_userinfo(
        &state.oidc,
        &state.http,
        &code,
        &flow.verifier,
        &flow.nonce,
    )
    .await
    {
        Ok(user) => user,
        Err(error) => {
            tracing::warn!(event="oidc.exchange_failed",%error);
            return (
                jar,
                html_page(
                    StatusCode::BAD_GATEWAY,
                    "Login failed",
                    "Identity verification failed. Please sign in again.",
                ),
            )
                .into_response();
        }
    };
    super::oauth::complete_login(state, jar, user, flow.next).await
}

async fn consume(
    state: &AppState,
    id: Uuid,
    hash: &str,
) -> notegate_core::Result<Option<LoginState>> {
    let Some(value) = state
        .oauth
        .store
        .consume_login_flow(id, hash, state.oidc.issuer(), Utc::now())
        .await?
    else {
        return Ok(None);
    };
    let encrypted = serde_json::from_value(value)
        .map_err(|_| notegate_core::Error::internal("invalid OIDC state"))?;
    let plaintext = state
        .security
        .decrypt_oauth_state(&id.to_string(), &encrypted)?;
    serde_json::from_str(&plaintext)
        .map(Some)
        .map_err(|_| notegate_core::Error::internal("invalid OIDC state"))
}
fn cookie_name(id: Uuid) -> String {
    format!("notegate_login_{id}")
}
fn invalid() -> Response {
    html_page(
        StatusCode::BAD_REQUEST,
        "Login failed",
        "The login request expired or was already used.",
    )
}
