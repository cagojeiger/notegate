//! Access category (owner-only): list / grant-or-change / revoke workspace access.
//!
//! `GET /api/v1/workspaces/{workspace_id}/access` (paginated, default and max
//! 100), `PUT .../access/{account_id}` to grant or change a role, and
//! `DELETE .../access/{account_id}` to revoke. The access service requires the
//! caller to be `owner` (no role ⇒ 404, lesser role ⇒ 403).

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_model::{Caller, WorkspaceAccess};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::rest::dto::{AccountRef, Page, parse_role};
use crate::state::AppState;

use notegate_service::access::{GrantAccess, ListAccess};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/workspaces/{workspace_id}/access", get(list))
        .route(
            "/v1/workspaces/{workspace_id}/access/{account_id}",
            axum::routing::put(grant).delete(revoke),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AccessOut {
    account: AccountRef,
    role: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ListResponse {
    access: Vec<AccessOut>,
    page: Page,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct GrantBody {
    role: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{workspace_id}/access",
    tag = "access",
    params(("workspace_id" = Uuid, Path)),
    responses((status = 200, description = "List access", body = ListResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<ListResponse>, ApiError> {
    let page = state
        .access
        .list_page(
            caller.account_id(),
            workspace_id,
            ListAccess {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;

    let ids: Vec<Uuid> = page.items.iter().map(|grant| grant.account_id).collect();
    let refs = state.accounts.find_account_refs(&ids).await?;
    let access = page
        .items
        .iter()
        .map(|grant: &WorkspaceAccess| AccessOut {
            account: AccountRef::resolve(grant.account_id, &refs),
            role: grant.role.as_str().to_owned(),
            created_at: grant.created_at,
        })
        .collect();
    Ok(Json(ListResponse {
        access,
        page: Page {
            limit: page.limit,
            returned: page.items.len() as i64,
            has_more: page.has_more,
            next_cursor: page.next_cursor,
        },
    }))
}

#[utoipa::path(
    put,
    path = "/api/v1/workspaces/{workspace_id}/access/{account_id}",
    tag = "access",
    params(("workspace_id" = Uuid, Path), ("account_id" = Uuid, Path)),
    request_body = GrantBody,
    responses((status = 200, description = "Grant or change access", body = AccessOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn grant(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((workspace_id, account_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<GrantBody>,
) -> Result<Json<AccessOut>, ApiError> {
    let role = parse_role(&body.role)?;
    let grant = state
        .access
        .grant(
            caller.account_id(),
            GrantAccess {
                workspace_id,
                account_id,
                role,
            },
        )
        .await?;
    let refs = state.accounts.find_account_refs(&[account_id]).await?;
    Ok(Json(AccessOut {
        account: AccountRef::resolve(grant.account_id, &refs),
        role: grant.role.as_str().to_owned(),
        created_at: grant.created_at,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/workspaces/{workspace_id}/access/{account_id}",
    tag = "access",
    params(("workspace_id" = Uuid, Path), ("account_id" = Uuid, Path)),
    responses((status = 204, description = "Revoke access")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn revoke(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((workspace_id, account_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state
        .access
        .revoke(caller.account_id(), workspace_id, account_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
