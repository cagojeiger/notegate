//! Owner-only dashboard trash; external API/MCP/CLI routes do not expose it.
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_model::Caller;
use notegate_model::trash::TrashItem;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::set_private_no_store;
use crate::error::ApiError;
use crate::page::Page;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/me/trash", get(list))
        .route(
            "/v1/me/trash/spaces/{space_id}/restore",
            post(restore_space),
        )
        .route("/v1/me/trash/spaces/{space_id}", delete(purge_space))
        .route(
            "/v1/me/trash/spaces/{space_id}/nodes/{node_id}/restore",
            post(restore_node),
        )
        .route(
            "/v1/me/trash/spaces/{space_id}/nodes/{node_id}",
            delete(purge_node),
        )
}

#[derive(Deserialize)]
pub(crate) struct ListTrashQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct TrashItemOut {
    id: Uuid,
    space_id: Uuid,
    space_name: String,
    kind: String,
    name: String,
    path: String,
    deleted_at: DateTime<Utc>,
    purge_after: DateTime<Utc>,
    recoverable: bool,
    deletion_pending: bool,
}

impl From<TrashItem> for TrashItemOut {
    fn from(item: TrashItem) -> Self {
        Self {
            id: item.id,
            space_id: item.space_id,
            space_name: item.space_name,
            kind: item.kind,
            name: item.name,
            path: item.path,
            deleted_at: item.deleted_at,
            purge_after: item.purge_after,
            recoverable: item.recoverable,
            deletion_pending: item.deletion_pending,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub(crate) struct TrashListOut {
    items: Vec<TrashItemOut>,
    page: Page,
}

#[utoipa::path(
    get, path = "/api/v1/me/trash", tag = "trash",
    params(("limit" = Option<i64>, Query), ("cursor" = Option<String>, Query)),
    responses((status = 200, body = TrashListOut)), security(("browser_session" = []))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListTrashQuery>,
) -> Result<Response, ApiError> {
    let result = state
        .files
        .list_trash(&caller, query.limit, query.cursor.as_deref())
        .await?;
    let page = Page::from_items(
        result.limit,
        &result.items,
        result.has_more,
        result.next_cursor,
    );
    let mut response = Json(TrashListOut {
        items: result.items.into_iter().map(Into::into).collect(),
        page,
    })
    .into_response();
    set_private_no_store(response.headers_mut());
    Ok(response)
}

#[utoipa::path(
    post, path = "/api/v1/me/trash/spaces/{space_id}/nodes/{node_id}/restore", tag = "trash",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses((status = 204, description = "Restored")), security(("browser_session" = []))
)]
pub(crate) async fn restore_node(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space, node)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state
        .files
        .restore_trash(&caller, space, Some(node))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/api/v1/me/trash/spaces/{space_id}/restore", tag = "trash",
    params(("space_id" = Uuid, Path)),
    responses((status = 204, description = "Restored; agent connections require reconnection")), security(("browser_session" = []))
)]
pub(crate) async fn restore_space(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.files.restore_trash(&caller, space, None).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub(crate) struct PurgeRequestedOut {
    status: &'static str,
}

#[utoipa::path(
    delete, path = "/api/v1/me/trash/spaces/{space_id}/nodes/{node_id}", tag = "trash",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses((status = 202, description = "Deletion queued, not completed", body = PurgeRequestedOut)), security(("browser_session" = []))
)]
pub(crate) async fn purge_node(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space, node)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<PurgeRequestedOut>), ApiError> {
    state.files.purge_trash(&caller, space, Some(node)).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(PurgeRequestedOut {
            status: "deletion_requested",
        }),
    ))
}

#[utoipa::path(
    delete, path = "/api/v1/me/trash/spaces/{space_id}", tag = "trash",
    params(("space_id" = Uuid, Path)),
    responses((status = 202, description = "Deletion queued, not completed", body = PurgeRequestedOut)), security(("browser_session" = []))
)]
pub(crate) async fn purge_space(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space): Path<Uuid>,
) -> Result<(StatusCode, Json<PurgeRequestedOut>), ApiError> {
    state.files.purge_trash(&caller, space, None).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(PurgeRequestedOut {
            status: "deletion_requested",
        }),
    ))
}
