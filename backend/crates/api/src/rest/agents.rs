//! Agents category: agent account and key lifecycle.
//!
//! `GET /api/v1/agents` (created-by-caller only, paginated default/max 100),
//! `POST` to create, `DELETE /{agent_id}`, `POST /{agent_id}/keys` (returns the
//! plaintext key exactly once), and `DELETE /{agent_id}/keys/{key_id}`.
//!
//! LOCKED: only `kind='user'` callers may list/manage agents/keys; the agents service
//! owns ownership, active-account, and lifecycle checks. `GET /agents` returns
//! active agents created by the caller only. An agent the caller did not create
//! is reported as not-found (`404`).

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_model::{Agent, ApiKey, Caller, ListApiKeys};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::page::Page;
use crate::state::AppState;

use notegate_service::agents::{CreateAgent, CreateAgentKey, ListAgents};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/agents", get(list).post(create))
        .route("/v1/agents/{agent_id}", axum::routing::delete(delete_agent))
        .route(
            "/v1/agents/{agent_id}/keys",
            get(list_keys).post(create_key),
        )
        .route(
            "/v1/agents/{agent_id}/keys/{key_id}",
            post(rotate_key).delete(revoke_key),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AgentOut {
    id: Uuid,
    name: String,
    created_by: Uuid,
}

impl From<&Agent> for AgentOut {
    fn from(agent: &Agent) -> Self {
        Self {
            id: agent.id,
            name: agent.name.clone(),
            created_by: agent.created_by,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AgentsListResponse {
    agents: Vec<AgentOut>,
    page: Page,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CreateAgentBody {
    name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CreateKeyBody {
    name: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
}

/// The one-time key creation response: metadata plus the plaintext token, which
/// is returned exactly once and never stored.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CreatedKeyOut {
    id: Uuid,
    agent_id: Uuid,
    name: String,
    scopes: Vec<String>,
    expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    /// The plaintext key. Shown once; store it now.
    token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct KeyOut {
    id: Uuid,
    account_id: Uuid,
    name: String,
    scopes: Vec<String>,
    expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl From<&ApiKey> for KeyOut {
    fn from(key: &ApiKey) -> Self {
        Self {
            id: key.id,
            account_id: key.account_id,
            name: key.name.clone(),
            scopes: key.scopes.clone(),
            expires_at: key.expires_at,
            created_at: key.created_at,
            revoked_at: key.revoked_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct KeysListResponse {
    keys: Vec<KeyOut>,
    page: Page,
}

#[utoipa::path(
    get,
    path = "/api/v1/agents",
    tag = "agents",
    params(
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List agents", body = AgentsListResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListQuery>,
) -> Result<Json<AgentsListResponse>, ApiError> {
    let page = state
        .agents
        .list_agents_page(
            caller.account.kind,
            caller.account_id(),
            ListAgents {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let agents = page.items.iter().map(AgentOut::from).collect();
    Ok(Json(AgentsListResponse {
        agents,
        page: Page::new(
            page.limit,
            page.items.len(),
            page.has_more,
            page.next_cursor,
        ),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/agents",
    tag = "agents",
    request_body = CreateAgentBody,
    responses((status = 201, description = "Create agent", body = AgentOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<CreateAgentBody>,
) -> Result<(StatusCode, Json<AgentOut>), ApiError> {
    let agent = state
        .agents
        .create_agent(
            caller.account.kind,
            caller.account_id(),
            CreateAgent { name: body.name },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(AgentOut::from(&agent))))
}

#[utoipa::path(
    delete,
    path = "/api/v1/agents/{agent_id}",
    tag = "agents",
    params(("agent_id" = Uuid, Path)),
    responses((status = 204, description = "Delete agent")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn delete_agent(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(agent_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .agents
        .delete_agent(caller.account.kind, caller.account_id(), agent_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/agents/{agent_id}/keys",
    tag = "agents",
    params(
        ("agent_id" = Uuid, Path),
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List agent keys", body = KeysListResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list_keys(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(agent_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<KeysListResponse>, ApiError> {
    let page = state
        .agents
        .list_keys(
            caller.account.kind,
            caller.account_id(),
            agent_id,
            ListApiKeys {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let keys = page.items.iter().map(KeyOut::from).collect();
    Ok(Json(KeysListResponse {
        keys,
        page: Page::new(
            page.limit,
            page.items.len(),
            page.has_more,
            page.next_cursor,
        ),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/agents/{agent_id}/keys",
    tag = "agents",
    params(("agent_id" = Uuid, Path)),
    request_body = CreateKeyBody,
    responses((status = 201, description = "Create agent key", body = CreatedKeyOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn create_key(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(agent_id): Path<Uuid>,
    Json(body): Json<CreateKeyBody>,
) -> Result<(StatusCode, Json<CreatedKeyOut>), ApiError> {
    let minted = state
        .agents
        .create_key(
            caller.account.kind,
            caller.account_id(),
            CreateAgentKey {
                agent_id,
                name: body.name,
                scopes: body.scopes,
                expires_at: body.expires_at,
            },
        )
        .await?;
    let key = minted.key;
    Ok((
        StatusCode::CREATED,
        Json(CreatedKeyOut {
            id: key.id,
            agent_id: key.account_id,
            name: key.name,
            scopes: key.scopes,
            expires_at: key.expires_at,
            created_at: key.created_at,
            token: minted.token,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/agents/{agent_id}/keys/{key_id}",
    tag = "agents",
    params(("agent_id" = Uuid, Path), ("key_id" = Uuid, Path)),
    responses((status = 201, description = "Rotate agent key", body = CreatedKeyOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn rotate_key(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((agent_id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<CreatedKeyOut>), ApiError> {
    let minted = state
        .agents
        .rotate_key(caller.account.kind, caller.account_id(), agent_id, key_id)
        .await?;
    let key = minted.key;
    Ok((
        StatusCode::CREATED,
        Json(CreatedKeyOut {
            id: key.id,
            agent_id: key.account_id,
            name: key.name,
            scopes: key.scopes,
            expires_at: key.expires_at,
            created_at: key.created_at,
            token: minted.token,
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/agents/{agent_id}/keys/{key_id}",
    tag = "agents",
    params(("agent_id" = Uuid, Path), ("key_id" = Uuid, Path)),
    responses((status = 204, description = "Revoke agent key")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn revoke_key(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((agent_id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state
        .agents
        .revoke_key(caller.account.kind, caller.account_id(), agent_id, key_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
