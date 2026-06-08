//! Workspaces category: list / create / get / rename / delete.
//!
//! `GET /api/v1/workspaces` (paginated, default 50, max 100), `POST` to create,
//! and `GET`/`PATCH`/`DELETE /{workspace_id}`. Each handler resolves the caller
//! from the auth middleware and delegates to the workspace service, which owns
//! authorization (no live role ⇒ 404, lesser role ⇒ 403).

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use notegate_model::Caller;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::page::Page;
use crate::rest::dto::WorkspaceOut;
use crate::state::AppState;

use notegate_service::workspaces::{CreateWorkspace, ListWorkspaces, RenameWorkspace};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/workspaces", get(list).post(create))
        .route(
            "/v1/workspaces/{workspace_id}",
            get(get_one).patch(rename).delete(delete),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ListResponse {
    workspaces: Vec<WorkspaceOut>,
    page: Page,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CreateBody {
    name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct RenameBody {
    name: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/workspaces",
    tag = "workspaces",
    responses((status = 200, description = "List workspaces", body = ListResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListQuery>,
) -> Result<Json<ListResponse>, ApiError> {
    let page = state
        .workspaces
        .list(
            caller.account_id(),
            ListWorkspaces {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let workspaces = page.items.iter().map(WorkspaceOut::from).collect();
    Ok(Json(ListResponse {
        workspaces,
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
    path = "/api/v1/workspaces",
    tag = "workspaces",
    request_body = CreateBody,
    responses((status = 201, description = "Create workspace", body = WorkspaceOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<WorkspaceOut>), ApiError> {
    let view = state
        .workspaces
        .create(
            caller.account.kind,
            caller.account_id(),
            CreateWorkspace { name: body.name },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(WorkspaceOut::from(&view))))
}

#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{workspace_id}",
    tag = "workspaces",
    params(("workspace_id" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, description = "Get workspace", body = WorkspaceOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn get_one(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<WorkspaceOut>, ApiError> {
    let view = state
        .workspaces
        .get(caller.account_id(), workspace_id)
        .await?;
    Ok(Json(WorkspaceOut::from(&view)))
}

#[utoipa::path(
    patch,
    path = "/api/v1/workspaces/{workspace_id}",
    tag = "workspaces",
    params(("workspace_id" = Uuid, Path, description = "Workspace id")),
    request_body = RenameBody,
    responses((status = 200, description = "Rename workspace", body = WorkspaceOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn rename(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<RenameBody>,
) -> Result<Json<WorkspaceOut>, ApiError> {
    let view = state
        .workspaces
        .rename(
            caller.account_id(),
            RenameWorkspace {
                workspace_id,
                new_name: body.name,
            },
        )
        .await?;
    Ok(Json(WorkspaceOut::from(&view)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/workspaces/{workspace_id}",
    tag = "workspaces",
    params(("workspace_id" = Uuid, Path, description = "Workspace id")),
    responses((status = 204, description = "Delete workspace")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn delete(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(workspace_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .workspaces
        .delete(caller.account_id(), workspace_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
