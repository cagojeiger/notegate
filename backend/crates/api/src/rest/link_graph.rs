use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_model::{Caller, LinkReference, LinkReferencePage, ListLinkReferences};
use notegate_service::link_graph::LinkGraphSpaceRequestOutcome;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::page::Page;
use crate::rest::dto::AsyncOperationResponse;
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
            "/v1/spaces/{space_id}/link-index/reindex",
            post(reindex_space),
        )
        .route(
            "/v1/spaces/{space_id}/link-index/status",
            get(get_space_link_index_status),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct LinkReferencesQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct NodeLinksResponse {
    status: &'static str,
    space_pending: bool,
    projected_at: Option<DateTime<Utc>>,
    failure_code: Option<String>,
    failed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LinkReferenceOut {
    node_id: Option<Uuid>,
    path: String,
    kind: &'static str,
    occurrence_count: i32,
}

impl From<LinkReference> for LinkReferenceOut {
    fn from(reference: LinkReference) -> Self {
        Self {
            node_id: reference.node_id,
            path: reference.path,
            kind: reference.kind.as_str(),
            occurrence_count: reference.occurrence_count,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LinkReferencesResponse {
    links: Vec<LinkReferenceOut>,
    page: Page,
}

impl From<LinkReferencePage> for LinkReferencesResponse {
    fn from(page: LinkReferencePage) -> Self {
        let links = page.items.into_iter().map(Into::into).collect::<Vec<_>>();
        let pagination = Page::from_items(page.limit, &links, page.has_more, page.next_cursor);
        Self {
            links,
            page: pagination,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SpaceLinkIndexStatusResponse {
    pending: bool,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/link-index/status",
    tag = "links",
    params(("space_id" = Uuid, Path, description = "Space id")),
    responses((status = 200, description = "Get Space link index work state", body = SpaceLinkIndexStatusResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn get_space_link_index_status(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<Json<SpaceLinkIndexStatusResponse>, ApiError> {
    let pending = state.link_graph.space_pending(&caller, space_id).await?;
    Ok(Json(SpaceLinkIndexStatusResponse { pending }))
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/links",
    tag = "links",
    params(
        ("space_id" = Uuid, Path, description = "Space id"),
        ("node_id" = Uuid, Path, description = "Node id"),
    ),
    responses((status = 200, description = "Get link projection state", body = NodeLinksResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn get_node_links(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<NodeLinksResponse>, ApiError> {
    let graph_state = state
        .link_graph
        .node_state(caller.account_id(), space_id, node_id)
        .await?;
    Ok(Json(NodeLinksResponse {
        status: graph_state.status.as_str(),
        space_pending: graph_state.space_pending,
        projected_at: graph_state.projected_at,
        failure_code: graph_state.failure_code,
        failed_at: graph_state.failed_at,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/links/outgoing",
    tag = "links",
    params(
        ("space_id" = Uuid, Path, description = "Space id"),
        ("node_id" = Uuid, Path, description = "Node id"),
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List outgoing links", body = LinkReferencesResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn get_outgoing_links(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<LinkReferencesQuery>,
) -> Result<Json<LinkReferencesResponse>, ApiError> {
    let page = state
        .link_graph
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
    Ok(Json(page.into()))
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/links/incoming",
    tag = "links",
    params(
        ("space_id" = Uuid, Path, description = "Space id"),
        ("node_id" = Uuid, Path, description = "Node id"),
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List incoming links", body = LinkReferencesResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn get_incoming_links(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<LinkReferencesQuery>,
) -> Result<Json<LinkReferencesResponse>, ApiError> {
    let page = state
        .link_graph
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
    Ok(Json(page.into()))
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/links/sync",
    tag = "links",
    params(
        ("space_id" = Uuid, Path, description = "Space id"),
        ("node_id" = Uuid, Path, description = "Node id"),
    ),
    responses((status = 202, description = "Accept node link synchronization", body = AsyncOperationResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn sync_node_links(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<AsyncOperationResponse>), ApiError> {
    state
        .link_graph
        .request_node(&caller, space_id, node_id)
        .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(AsyncOperationResponse::accepted(None)),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/link-index/reindex",
    tag = "links",
    params(("space_id" = Uuid, Path, description = "Space id")),
    responses((status = 202, description = "Accept full Space link reindex", body = AsyncOperationResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn reindex_space(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<(StatusCode, Json<AsyncOperationResponse>), ApiError> {
    let response = match state.link_graph.request_space(&caller, space_id).await? {
        LinkGraphSpaceRequestOutcome::Requested => AsyncOperationResponse::accepted(None),
        LinkGraphSpaceRequestOutcome::AlreadyPending => {
            AsyncOperationResponse::already_pending(None)
        }
    };
    Ok((StatusCode::ACCEPTED, Json(response)))
}
