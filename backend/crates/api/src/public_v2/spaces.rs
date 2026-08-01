use axum::extract::{Extension, Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use notegate_model::Caller;
use notegate_service::spaces::ListSpaces;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

use super::dto::{PageOut, SpaceOut};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/spaces", get(list))
        .route("/spaces/{space_id}", get(get_one))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SpacesResponse {
    spaces: Vec<SpaceOut>,
    page: PageOut,
}

#[utoipa::path(
    get,
    path = "/api/v2/spaces",
    tag = "spaces",
    params(
        ("limit" = Option<i64>, Query, description = "Page size; defaults to 50 and is capped at 100"),
        ("cursor" = Option<String>, Query, description = "Opaque cursor returned by the preceding response; keep all other parameters unchanged"),
    ),
    responses((status = 200, description = "List spaces connected to the Agent", body = SpacesResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListQuery>,
) -> Result<Json<SpacesResponse>, ApiError> {
    let page = state
        .spaces
        .list_mcp(
            caller.account_id(),
            ListSpaces {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let spaces = page.items.iter().map(SpaceOut::from).collect::<Vec<_>>();
    Ok(Json(SpacesResponse {
        page: PageOut::new(page.limit, spaces.len(), page.has_more, page.next_cursor),
        spaces,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v2/spaces/{space_id}",
    tag = "spaces",
    params(("space_id" = Uuid, Path, description = "Space id")),
    responses((status = 200, description = "Get a connected space", body = SpaceOut)),
    security(("api_key" = []))
)]
pub(crate) async fn get_one(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<Json<SpaceOut>, ApiError> {
    let view = state
        .spaces
        .find_mcp_visible_by_id(caller.account_id(), space_id)
        .await?
        .ok_or_else(|| ApiError::not_found("space not found"))?;
    Ok(Json(SpaceOut::from(&view)))
}
