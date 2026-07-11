//! Spaces category: lifecycle and manual usage reconciliation requests.
//!
//! `GET /api/v1/spaces` (paginated, default 50, max 100), `POST` to create,
//! `GET`/`PATCH`/`DELETE /{space_id}`, and queued usage reconciliation. Each
//! handler resolves the caller
//! from the auth middleware and delegates to the space service, which owns
//! authorization (no live permission ⇒ 404, insufficient permission ⇒ 403).

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use notegate_model::Caller;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::page::Page;
use crate::rest::dto::SpaceOut;
use crate::state::AppState;

use notegate_service::spaces::{CreateSpace, ListSpaces, UpdateSpace};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces", get(list).post(create))
        .route(
            "/v1/spaces/{space_id}",
            get(get_one).patch(update).delete(delete),
        )
        .route(
            "/v1/spaces/{space_id}/usage/reconcile",
            post(request_usage_reconciliation),
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
pub(crate) struct UpdateBody {
    name: Option<String>,
    sort_order: Option<i32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ReconciliationQueuedResponse {
    status: &'static str,
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
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
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
    request_body = UpdateBody,
    responses((status = 200, description = "Update space", body = SpaceOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn update(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<SpaceOut>, ApiError> {
    let view = state
        .spaces
        .update(
            caller.account.kind,
            caller.account_id(),
            UpdateSpace {
                space_id,
                name: body.name,
                sort_order: body.sort_order,
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

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/usage/reconcile",
    tag = "spaces",
    params(("space_id" = Uuid, Path, description = "Space id")),
    responses((status = 202, description = "Queue usage reconciliation", body = ReconciliationQueuedResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn request_usage_reconciliation(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<(StatusCode, Json<ReconciliationQueuedResponse>), ApiError> {
    state
        .usage
        .request_space_reconciliation(caller.account.kind, caller.account_id(), space_id)
        .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ReconciliationQueuedResponse { status: "queued" }),
    ))
}
