//! Browser-only link-index state, rebuild command, and node relation reads.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_model::{
    Caller, LinkIndexFreshness, LinkIndexStatus, LinkReference, LinkReferenceKind,
    LinkReferenceStatus, NodeLinkSummary, SpaceLinkIndexState,
};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/{space_id}/link-index", get(get_state))
        .route(
            "/v1/spaces/{space_id}/link-index/rebuild",
            post(request_rebuild),
        )
        .route(
            "/v1/spaces/{space_id}/nodes/{node_id}/links",
            get(get_node_links),
        )
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LinkIndexStatusOut {
    Uninitialized,
    Queued,
    Running,
    Rebuilding,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LinkIndexFreshnessOut {
    Uninitialized,
    Current,
    Updating,
    Rebuilding,
    Failed,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LinkIndexStateOut {
    space_id: Uuid,
    desired_generation: i64,
    applied_generation: i64,
    status: LinkIndexStatusOut,
    freshness: LinkIndexFreshnessOut,
    last_indexed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LinkReferenceKindOut {
    Link,
    Image,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LinkReferenceStatusOut {
    Resolved,
    Deleted,
    Missing,
    Invalid,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LinkReferenceOut {
    id: i64,
    kind: LinkReferenceKindOut,
    status: LinkReferenceStatusOut,
    raw_href: String,
    normalized_target_path: Option<String>,
    occurrence_count: i32,
    source_node_id: Uuid,
    source_name: String,
    source_path: Option<String>,
    target_node_id: Option<Uuid>,
    target_name: Option<String>,
    target_path: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct NodeLinkSummaryOut {
    index: LinkIndexStateOut,
    outgoing_count: i64,
    incoming_count: i64,
    broken_count: i64,
    outgoing: Vec<LinkReferenceOut>,
    incoming: Vec<LinkReferenceOut>,
    outgoing_truncated: bool,
    incoming_truncated: bool,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/link-index",
    tag = "links",
    params(("space_id" = Uuid, Path, description = "Space id")),
    responses((status = 200, description = "Get link-index state", body = LinkIndexStateOut)),
    security(("browser_session" = []))
)]
pub(crate) async fn get_state(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<Json<LinkIndexStateOut>, ApiError> {
    let index = state
        .link_index
        .state(caller.account_id(), space_id)
        .await?;
    Ok(Json(index.into()))
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/link-index/rebuild",
    tag = "links",
    params(("space_id" = Uuid, Path, description = "Space id")),
    responses(
        (status = 202, description = "Queue a full Space link reindex", body = LinkIndexStateOut),
        (status = 403, description = "Write permission is required", body = ErrorResponse),
    ),
    security(("browser_session" = []))
)]
pub(crate) async fn request_rebuild(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
) -> Result<(StatusCode, Json<LinkIndexStateOut>), ApiError> {
    let index = state
        .link_index
        .request_rebuild(caller.account_id(), space_id)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(index.into())))
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/links",
    tag = "links",
    params(
        ("space_id" = Uuid, Path, description = "Space id"),
        ("node_id" = Uuid, Path, description = "Node id"),
    ),
    responses((status = 200, description = "Get bounded incoming and outgoing relations", body = NodeLinkSummaryOut)),
    security(("browser_session" = []))
)]
pub(crate) async fn get_node_links(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<NodeLinkSummaryOut>, ApiError> {
    let summary = state
        .link_index
        .node_links(caller.account_id(), space_id, node_id)
        .await?;
    Ok(Json(summary.into()))
}

impl From<SpaceLinkIndexState> for LinkIndexStateOut {
    fn from(state: SpaceLinkIndexState) -> Self {
        Self {
            space_id: state.space_id,
            desired_generation: state.desired_generation,
            applied_generation: state.applied_generation,
            status: state.status.into(),
            freshness: state.freshness().into(),
            last_indexed_at: state.last_indexed_at,
        }
    }
}

impl From<LinkIndexStatus> for LinkIndexStatusOut {
    fn from(status: LinkIndexStatus) -> Self {
        match status {
            LinkIndexStatus::Uninitialized => Self::Uninitialized,
            LinkIndexStatus::Queued => Self::Queued,
            LinkIndexStatus::Running => Self::Running,
            LinkIndexStatus::Rebuilding => Self::Rebuilding,
            LinkIndexStatus::Ready => Self::Ready,
            LinkIndexStatus::Failed => Self::Failed,
        }
    }
}

impl From<LinkIndexFreshness> for LinkIndexFreshnessOut {
    fn from(freshness: LinkIndexFreshness) -> Self {
        match freshness {
            LinkIndexFreshness::Uninitialized => Self::Uninitialized,
            LinkIndexFreshness::Current => Self::Current,
            LinkIndexFreshness::Updating => Self::Updating,
            LinkIndexFreshness::Rebuilding => Self::Rebuilding,
            LinkIndexFreshness::Failed => Self::Failed,
        }
    }
}

impl From<LinkReferenceKind> for LinkReferenceKindOut {
    fn from(kind: LinkReferenceKind) -> Self {
        match kind {
            LinkReferenceKind::Link => Self::Link,
            LinkReferenceKind::Image => Self::Image,
        }
    }
}

impl From<LinkReferenceStatus> for LinkReferenceStatusOut {
    fn from(status: LinkReferenceStatus) -> Self {
        match status {
            LinkReferenceStatus::Resolved => Self::Resolved,
            LinkReferenceStatus::Deleted => Self::Deleted,
            LinkReferenceStatus::Missing => Self::Missing,
            LinkReferenceStatus::Invalid => Self::Invalid,
        }
    }
}

impl From<LinkReference> for LinkReferenceOut {
    fn from(reference: LinkReference) -> Self {
        Self {
            id: reference.id,
            kind: reference.kind.into(),
            status: reference.status.into(),
            raw_href: reference.raw_href,
            normalized_target_path: reference.normalized_target_path,
            occurrence_count: reference.occurrence_count,
            source_node_id: reference.source_node_id,
            source_name: reference.source_name,
            source_path: reference.source_path,
            target_node_id: reference.target_node_id,
            target_name: reference.target_name,
            target_path: reference.target_path,
        }
    }
}

impl From<NodeLinkSummary> for NodeLinkSummaryOut {
    fn from(summary: NodeLinkSummary) -> Self {
        Self {
            index: summary.index.into(),
            outgoing_count: summary.outgoing_count,
            incoming_count: summary.incoming_count,
            broken_count: summary.broken_count,
            outgoing: summary.outgoing.into_iter().map(Into::into).collect(),
            incoming: summary.incoming.into_iter().map(Into::into).collect(),
            outgoing_truncated: summary.outgoing_truncated,
            incoming_truncated: summary.incoming_truncated,
        }
    }
}
