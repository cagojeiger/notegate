use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use notegate_model::{
    Caller, LinkReferencePage, LinkReferenceView, ListLinkReferences, NodeLinkIndexView,
    SpaceLinkIndexView,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::page::Page;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/spaces/{space_id}/nodes/{node_id}/links",
            get(get_node_links),
        )
        .route(
            "/v1/spaces/{space_id}/nodes/{node_id}/links/outgoing",
            get(get_outgoing_links),
        )
        .route(
            "/v1/spaces/{space_id}/nodes/{node_id}/links/incoming",
            get(get_incoming_links),
        )
        .route(
            "/v1/spaces/{space_id}/nodes/{node_id}/links/sync",
            post(sync_node_links),
        )
        .route(
            "/v1/spaces/{space_id}/link-index",
            get(get_space_link_index),
        )
        .route(
            "/v1/spaces/{space_id}/link-index/reindex",
            post(reindex_space),
        )
}

pub(crate) async fn get_node_links(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<NodeLinkIndexView>, ApiError> {
    let view = state
        .link_index
        .node(caller.account_id(), space_id, node_id)
        .await?;
    Ok(Json(view))
}

pub(crate) async fn get_outgoing_links(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<LinkReferencesQuery>,
) -> Result<Json<LinkReferencesResponse>, ApiError> {
    let page = state
        .link_index
        .outgoing(
            caller.account_id(),
            space_id,
            node_id,
            ListLinkReferences {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    Ok(Json(LinkReferencesResponse::from(page)))
}

pub(crate) async fn get_incoming_links(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<LinkReferencesQuery>,
) -> Result<Json<LinkReferencesResponse>, ApiError> {
    let page = state
        .link_index
        .incoming(
            caller.account_id(),
            space_id,
            node_id,
            ListLinkReferences {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    Ok(Json(LinkReferencesResponse::from(page)))
}

pub(crate) async fn sync_node_links(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<QueuedResponse>), ApiError> {
    state
        .link_index
        .request_node(&caller, space_id, node_id)
        .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(QueuedResponse { status: "queued" }),
    ))
}

pub(crate) async fn get_space_link_index(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<Json<SpaceLinkIndexView>, ApiError> {
    let view = state
        .link_index
        .space(caller.account_id(), space_id)
        .await?;
    Ok(Json(view))
}

pub(crate) async fn reindex_space(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<(StatusCode, Json<QueuedResponse>), ApiError> {
    state.link_index.request_space(&caller, space_id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(QueuedResponse { status: "queued" }),
    ))
}

#[derive(Debug, Serialize)]
pub(crate) struct QueuedResponse {
    status: &'static str,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LinkReferencesQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LinkReferencesResponse {
    links: Vec<LinkReferenceView>,
    page: Page,
}

impl From<LinkReferencePage> for LinkReferencesResponse {
    fn from(value: LinkReferencePage) -> Self {
        let LinkReferencePage {
            items,
            limit,
            has_more,
            next_cursor,
        } = value;
        let page = Page::from_items(limit, &items, has_more, next_cursor);
        Self { links: items, page }
    }
}
