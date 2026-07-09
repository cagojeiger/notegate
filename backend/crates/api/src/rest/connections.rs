//! Space agent connections (user owner only): list / connect / disconnect.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_model::{Caller, Permission, SpaceAgentConnection};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::page::Page;
use crate::rest::dto::AccountRef;
use crate::state::AppState;

use notegate_service::connections::{ConnectAgent, ListConnections};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/{space_id}/agents", get(list))
        .route(
            "/v1/spaces/{space_id}/agents/{agent_id}",
            axum::routing::put(connect).delete(disconnect),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ConnectionOut {
    agent: AccountRef,
    permission: String,
    connected_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ConnectionListResponse {
    connections: Vec<ConnectionOut>,
    page: Page,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PermissionBody {
    Read,
    Write,
}

impl From<PermissionBody> for Permission {
    fn from(value: PermissionBody) -> Self {
        match value {
            PermissionBody::Read => Self::Read,
            PermissionBody::Write => Self::Write,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct ConnectBody {
    permission: PermissionBody,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/agents",
    tag = "connections",
    params(
        ("space_id" = Uuid, Path),
        ("limit" = Option<i64>, Query),
        ("cursor" = Option<String>, Query),
    ),
    responses((status = 200, description = "List agent connections", body = ConnectionListResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<ConnectionListResponse>, ApiError> {
    let page = state
        .connections
        .list_page(
            caller.account.kind,
            caller.account_id(),
            space_id,
            ListConnections {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;

    let ids: Vec<Uuid> = page
        .items
        .iter()
        .map(|connection| connection.agent_id)
        .collect();
    let refs = state.accounts.find_account_refs(&ids).await?;
    let connections = page
        .items
        .iter()
        .map(|connection: &SpaceAgentConnection| ConnectionOut {
            agent: AccountRef::resolve(connection.agent_id, &refs),
            permission: connection.permission.as_str().to_owned(),
            connected_at: connection.connected_at,
        })
        .collect();
    Ok(Json(ConnectionListResponse {
        connections,
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    }))
}

#[utoipa::path(
    put,
    path = "/api/v1/spaces/{space_id}/agents/{agent_id}",
    tag = "connections",
    params(("space_id" = Uuid, Path), ("agent_id" = Uuid, Path)),
    request_body = ConnectBody,
    responses((status = 200, description = "Connect or update agent", body = ConnectionOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn connect(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ConnectBody>,
) -> Result<Json<ConnectionOut>, ApiError> {
    let connection = state
        .connections
        .connect(
            caller.account.kind,
            caller.account_id(),
            ConnectAgent {
                space_id,
                agent_id,
                permission: Permission::from(body.permission),
            },
        )
        .await?;
    let refs = state.accounts.find_account_refs(&[agent_id]).await?;
    Ok(Json(ConnectionOut {
        agent: AccountRef::resolve(connection.agent_id, &refs),
        permission: connection.permission.as_str().to_owned(),
        connected_at: connection.connected_at,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/spaces/{space_id}/agents/{agent_id}",
    tag = "connections",
    params(("space_id" = Uuid, Path), ("agent_id" = Uuid, Path)),
    responses((status = 204, description = "Disconnect agent")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn disconnect(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state
        .connections
        .disconnect(caller.account.kind, caller.account_id(), space_id, agent_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
