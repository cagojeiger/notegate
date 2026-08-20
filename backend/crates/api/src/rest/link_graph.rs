use axum::extract::{Extension, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_model::{
    Caller, LinkReference, LinkReferencePage, ListLinkReferences, NodeLinkGraphStatus,
};
use notegate_service::link_graph::{
    LinkGraphNodeRequestOutcome, LinkGraphRequestEligibility, LinkGraphSpaceRequestOutcome,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::page::Page;
use crate::rest::dto::{AsyncCommandAck, CommandAvailability};
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
            "/v1/spaces/{space_id}/nodes/{node_id}/actions/reindex-links",
            post(sync_node_links),
        )
        .route(
            "/v1/spaces/{space_id}/actions/reindex-links",
            post(reindex_space),
        )
        .route(
            "/v1/spaces/{space_id}/link-index",
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
    availability: CommandAvailability,
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
    status: SpaceLinkIndexStatus,
    availability: CommandAvailability,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpaceLinkIndexStatus {
    Idle,
    Pending,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/link-index",
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
    let view = state.link_graph.space_view(&caller, space_id).await?;
    Ok(Json(SpaceLinkIndexStatusResponse {
        status: if view.pending {
            SpaceLinkIndexStatus::Pending
        } else {
            SpaceLinkIndexStatus::Idle
        },
        availability: command_availability(view.request_eligibility, view.pending),
    }))
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
    let view = state
        .link_graph
        .node_view(&caller, space_id, node_id)
        .await?;
    let active = matches!(
        view.state.status,
        NodeLinkGraphStatus::Pending | NodeLinkGraphStatus::Syncing
    );
    let availability =
        command_availability(view.request_eligibility, view.request_pending || active);
    Ok(Json(NodeLinksResponse {
        status: view.state.status.as_str(),
        space_pending: view.state.space_pending,
        projected_at: view.state.projected_at,
        failure_code: view.state.failure_code,
        failed_at: view.state.failed_at,
        availability,
    }))
}

fn command_availability(
    eligibility: LinkGraphRequestEligibility,
    pending: bool,
) -> CommandAvailability {
    match eligibility {
        LinkGraphRequestEligibility::Forbidden => CommandAvailability::forbidden(),
        LinkGraphRequestEligibility::Unsupported => CommandAvailability::unsupported(),
        LinkGraphRequestEligibility::Available if pending => CommandAvailability::pending(),
        LinkGraphRequestEligibility::Available => CommandAvailability::available(),
    }
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
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/actions/reindex-links",
    tag = "links",
    params(
        ("space_id" = Uuid, Path, description = "Space id"),
        ("node_id" = Uuid, Path, description = "Node id"),
    ),
    responses((
        status = 202,
        description = "Accept node link synchronization",
        body = AsyncCommandAck,
        headers(("Location" = String, description = "Node link state resource"))
    )),
    security(("browser_session" = []))
)]
pub(crate) async fn sync_node_links(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, String); 1],
        Json<AsyncCommandAck>,
    ),
    ApiError,
> {
    let response = match state
        .link_graph
        .request_node(&caller, space_id, node_id)
        .await?
    {
        LinkGraphNodeRequestOutcome::Requested => AsyncCommandAck::accepted(),
        LinkGraphNodeRequestOutcome::AlreadyPending => AsyncCommandAck::already_pending(),
    };
    Ok((
        StatusCode::ACCEPTED,
        [(
            header::LOCATION,
            format!("/api/v1/spaces/{space_id}/nodes/{node_id}/links"),
        )],
        Json(response),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/actions/reindex-links",
    tag = "links",
    params(("space_id" = Uuid, Path, description = "Space id")),
    responses((
        status = 202,
        description = "Accept full Space link reindex",
        body = AsyncCommandAck,
        headers(("Location" = String, description = "Space link index state resource"))
    )),
    security(("browser_session" = []))
)]
pub(crate) async fn reindex_space(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, String); 1],
        Json<AsyncCommandAck>,
    ),
    ApiError,
> {
    let response = match state.link_graph.request_space(&caller, space_id).await? {
        LinkGraphSpaceRequestOutcome::Requested => AsyncCommandAck::accepted(),
        LinkGraphSpaceRequestOutcome::AlreadyPending => AsyncCommandAck::already_pending(),
    };
    Ok((
        StatusCode::ACCEPTED,
        [(
            header::LOCATION,
            format!("/api/v1/spaces/{space_id}/link-index"),
        )],
        Json(response),
    ))
}
