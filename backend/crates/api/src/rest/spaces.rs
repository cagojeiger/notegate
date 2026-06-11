//! Spaces category: list / create / get / rename / delete.
//!
//! `GET /api/v1/spaces` (paginated, default 50, max 100), `POST` to create,
//! and `GET`/`PATCH`/`DELETE /{space_id}`. Each handler resolves the caller
//! from the auth middleware and delegates to the space service, which owns
//! authorization (no live permission ⇒ 404, insufficient permission ⇒ 403).

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
use crate::rest::dto::SpaceOut;
use crate::state::AppState;

use notegate_service::spaces::{CreateSpace, ListSpaces, RenameSpace};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces", get(list).post(create))
        .route(
            "/v1/spaces/{space_id}",
            get(get_one).patch(rename).delete(delete),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SpacesListResponse {
    spaces: Vec<SpaceOut>,
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
    path = "/api/v1/spaces",
    tag = "spaces",
    params(
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List spaces", body = SpacesListResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListQuery>,
) -> Result<Json<SpacesListResponse>, ApiError> {
    let page = state
        .spaces
        .list(
            caller.account_id(),
            ListSpaces {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let spaces = page.items.iter().map(SpaceOut::from).collect();
    Ok(Json(SpacesListResponse {
        spaces,
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
    path = "/api/v1/spaces",
    tag = "spaces",
    request_body = CreateBody,
    responses((status = 201, description = "Create space", body = SpaceOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<SpaceOut>), ApiError> {
    let view = state
        .spaces
        .create(
            caller.account.kind,
            caller.account_id(),
            CreateSpace { name: body.name },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(SpaceOut::from(&view))))
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}",
    tag = "spaces",
    params(("space_id" = Uuid, Path, description = "Space id")),
    responses((status = 200, description = "Get space", body = SpaceOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn get_one(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<Json<SpaceOut>, ApiError> {
    let view = state.spaces.get(caller.account_id(), space_id).await?;
    Ok(Json(SpaceOut::from(&view)))
}

#[utoipa::path(
    patch,
    path = "/api/v1/spaces/{space_id}",
    tag = "spaces",
    params(("space_id" = Uuid, Path, description = "Space id")),
    request_body = RenameBody,
    responses((status = 200, description = "Rename space", body = SpaceOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn rename(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Json(body): Json<RenameBody>,
) -> Result<Json<SpaceOut>, ApiError> {
    let view = state
        .spaces
        .rename(
            caller.account.kind,
            caller.account_id(),
            RenameSpace {
                space_id,
                new_name: body.name,
            },
        )
        .await?;
    Ok(Json(SpaceOut::from(&view)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/spaces/{space_id}",
    tag = "spaces",
    params(("space_id" = Uuid, Path, description = "Space id")),
    responses((status = 204, description = "Delete space")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn delete(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .spaces
        .delete(caller.account.kind, caller.account_id(), space_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
