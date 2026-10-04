//! Browser history endpoints; service owns permission checks and restore semantics.
use crate::{error::ApiError, page::Page, state::AppState};
use axum::{
    Json, Router,
    extract::{Extension, Path, Query, State},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use notegate_model::{Caller, text_revision::TextRevision};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/{space_id}/text/{node_id}/revisions", get(list))
        .route(
            "/v1/spaces/{space_id}/text/{node_id}/revisions/{revision_id}",
            get(read),
        )
        .route(
            "/v1/spaces/{space_id}/text/{node_id}/revisions/{revision_id}/restore",
            post(restore),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TextRevisionOut {
    id: Uuid,
    node_id: Uuid,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
    written_at: DateTime<Utc>,
    author_id: Uuid,
    group_id: Uuid,
    source: String,
    superseded_at: DateTime<Utc>,
}
impl From<TextRevision> for TextRevisionOut {
    fn from(r: TextRevision) -> Self {
        Self {
            id: r.id,
            node_id: r.node_id,
            content_sha256: r.content_sha256,
            byte_len: r.byte_len,
            line_count: r.line_count,
            written_at: r.written_at,
            author_id: r.author_id,
            group_id: r.group_id,
            source: r.source,
            superseded_at: r.superseded_at,
        }
    }
}
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TextRevisionListResponse {
    revisions: Vec<TextRevisionOut>,
    page: Page,
}
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TextRevisionReadResponse {
    revision: TextRevisionOut,
    content: String,
}
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextRevisionRestoreBody {
    /// Required hash of the currently displayed document; stale restores fail with 409.
    expected_sha256: String,
}
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TextRevisionRestoreResponse {
    node_id: Uuid,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
}

#[utoipa::path(get, path = "/api/v1/spaces/{space_id}/text/{node_id}/revisions", tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path),
        ("limit" = Option<i64>, Query, description = "1 to 100, default 50"), ("cursor" = Option<String>, Query)),
    responses((status = 200, description = "Historical metadata, newest first; current body is not included", body = TextRevisionListResponse)),
    security(("browser_session" = [])))]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space, node)): Path<(Uuid, Uuid)>,
    Query(query): Query<ListQuery>,
) -> Result<Json<TextRevisionListResponse>, ApiError> {
    let page = state
        .files
        .for_channel(caller.channel)
        .text_revisions(
            caller.account_id(),
            space,
            node,
            query.limit.unwrap_or(50),
            query.cursor.as_deref(),
        )
        .await?;
    let pagination = Page::from_items(
        query.limit.unwrap_or(50),
        &page.revisions,
        page.next_cursor.is_some(),
        page.next_cursor,
    );
    Ok(Json(TextRevisionListResponse {
        revisions: page.revisions.into_iter().map(Into::into).collect(),
        page: pagination,
    }))
}

#[utoipa::path(get, path = "/api/v1/spaces/{space_id}/text/{node_id}/revisions/{revision_id}", tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path), ("revision_id" = Uuid, Path)),
    responses((status = 200, description = "One historical body", body = TextRevisionReadResponse)), security(("browser_session" = [])))]
pub(crate) async fn read(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space, node, revision)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<TextRevisionReadResponse>, ApiError> {
    let value = state
        .files
        .for_channel(caller.channel)
        .text_revision(caller.account_id(), space, node, revision)
        .await?;
    Ok(Json(TextRevisionReadResponse {
        revision: value.revision.into(),
        content: value.content,
    }))
}

#[utoipa::path(post, path = "/api/v1/spaces/{space_id}/text/{node_id}/revisions/{revision_id}/restore", tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path), ("revision_id" = Uuid, Path)),
    request_body = TextRevisionRestoreBody,
    responses((status = 200, description = "Restore as a guarded normal save; same-body restore is a no-op", body = TextRevisionRestoreResponse),
        (status = 409, description = "Current content changed or revision storage is full")), security(("browser_session" = [])))]
pub(crate) async fn restore(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space, node, revision)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<TextRevisionRestoreBody>,
) -> Result<Json<TextRevisionRestoreResponse>, ApiError> {
    let view = state
        .files
        .for_channel(caller.channel)
        .restore_text_revision(
            caller.account_id(),
            space,
            node,
            revision,
            body.expected_sha256,
        )
        .await?;
    Ok(Json(TextRevisionRestoreResponse {
        node_id: node,
        content_sha256: view.text.content_sha256,
        byte_len: view.text.byte_len,
        line_count: view.text.line_count,
    }))
}
