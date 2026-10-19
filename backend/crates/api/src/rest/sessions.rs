use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use notegate_model::Caller;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::oauth::expired_browser_session_cookie;
use crate::auth::session::BROWSER_SESSION_COOKIE;
use crate::error::ApiError;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/me/sessions", get(list).delete(revoke_all))
        .route("/v1/me/sessions/others", delete(revoke_others))
        .route("/v1/me/sessions/{id}", delete(revoke_one))
        .route(
            "/v1/me/oauth-grants",
            get(list_grants).delete(revoke_grants),
        )
        .route("/v1/me/oauth-grants/{id}", delete(revoke_grant))
        .layer(axum::middleware::from_fn(
            crate::auth::mark_private_no_store,
        ))
}

#[derive(Serialize, ToSchema)]
pub(crate) struct SessionOut {
    id: Uuid,
    current: bool,
    client_environment: Option<String>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
    idle_expires_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct SessionList {
    sessions: Vec<SessionOut>,
}

fn current_id(jar: &CookieJar) -> Result<Uuid, ApiError> {
    jar.get(BROWSER_SESSION_COOKIE)
        .and_then(|cookie| notegate_db::browser_session_repo::parse_token(cookie.value()))
        .map(|(id, _)| id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "browser session required",
            )
        })
}

#[utoipa::path(get, path="/api/v1/me/sessions", tag="identity",
    responses((status=200, description="Latest 100 active browser sessions", body=SessionList)),
    security(("browser_session"=[])))]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    jar: CookieJar,
) -> Result<Json<SessionList>, ApiError> {
    let current = current_id(&jar)?;
    let sessions = state
        .browser_sessions
        .list_owned_live(caller.account_id(), state.oidc.issuer(), current)
        .await?
        .into_iter()
        .map(|session| SessionOut {
            id: session.id,
            current: session.id == current,
            client_environment: session.client_environment,
            created_at: session.created_at,
            last_used_at: session.last_used_at,
            idle_expires_at: session.idle_expires_at,
            expires_at: session.expires_at,
        })
        .collect();
    Ok(Json(SessionList { sessions }))
}

#[utoipa::path(delete, path="/api/v1/me/sessions/{id}", tag="identity",
    params(("id"=Uuid, Path)), responses((status=204, description="Revoke an owned browser session")),
    security(("browser_session"=[])))]
pub(crate) async fn revoke_one(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<Uuid>,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), ApiError> {
    let current = current_id(&jar)?;
    state
        .browser_sessions
        .revoke_owned_sessions(caller.account_id(), Some(id), None)
        .await?;
    let jar = if current == id {
        jar.add(expired_browser_session_cookie(state.config.secure_cookies))
    } else {
        jar
    };
    Ok((jar, StatusCode::NO_CONTENT))
}

#[utoipa::path(delete, path="/api/v1/me/sessions/others", tag="identity",
    responses((status=204, description="Revoke every other browser session")), security(("browser_session"=[])))]
pub(crate) async fn revoke_others(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    jar: CookieJar,
) -> Result<StatusCode, ApiError> {
    let current = current_id(&jar)?;
    state
        .browser_sessions
        .revoke_owned_sessions(caller.account_id(), None, Some(current))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path="/api/v1/me/sessions", tag="identity",
    responses((status=204, description="Revoke all browser sessions including this one")), security(("browser_session"=[])))]
pub(crate) async fn revoke_all(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), ApiError> {
    state
        .browser_sessions
        .revoke_owned_sessions(caller.account_id(), None, None)
        .await?;
    Ok((
        jar.add(expired_browser_session_cookie(state.config.secure_cookies)),
        StatusCode::NO_CONTENT,
    ))
}

#[derive(Serialize, ToSchema)]
pub(crate) struct GrantOut {
    id: Uuid,
    client_name: String,
    kind: &'static str,
    client_environment: Option<String>,
    created_at: DateTime<Utc>,
    last_token_at: Option<DateTime<Utc>>,
    idle_expires_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    offline_access: bool,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct GrantList {
    grants: Vec<GrantOut>,
}

#[utoipa::path(get, path="/api/v1/me/oauth-grants", tag="identity",
    responses((status=200, description="Active CLI and MCP OAuth connections", body=GrantList)),
    security(("browser_session"=[])))]
pub(crate) async fn list_grants(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<GrantList>, ApiError> {
    let grants = state
        .oauth
        .store
        .list_grants(caller.account_id(), Utc::now())
        .await?
        .into_iter()
        .map(|grant| GrantOut {
            kind: if grant.scope.split_whitespace().any(|s| s == "cli") {
                "cli"
            } else {
                "mcp"
            },
            offline_access: grant
                .scope
                .split_whitespace()
                .any(|s| s == "offline_access"),
            id: grant.id,
            client_name: grant.client_name,
            client_environment: grant.client_environment,
            created_at: grant.created_at,
            last_token_at: grant.last_token_at,
            idle_expires_at: grant.idle_expires_at,
            expires_at: grant.expires_at,
        })
        .collect();
    Ok(Json(GrantList { grants }))
}

#[utoipa::path(delete, path="/api/v1/me/oauth-grants/{id}", tag="identity",
    params(("id"=Uuid, Path)), responses((status=204, description="Revoke an owned connection and its access/refresh credentials")),
    security(("browser_session"=[])))]
pub(crate) async fn revoke_grant(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .oauth
        .store
        .revoke_owned_grant(id, caller.account_id(), Utc::now())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path="/api/v1/me/oauth-grants", tag="identity",
    responses((status=204, description="Revoke every CLI and MCP OAuth connection")), security(("browser_session"=[])))]
pub(crate) async fn revoke_grants(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<StatusCode, ApiError> {
    state
        .oauth
        .store
        .revoke_owned_grants(caller.account_id(), Utc::now())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
