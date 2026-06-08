//! Search category: `POST /search/find` and `POST /search/grep`.
//!
//! Workspace-scoped search. Authorization is checked once by the search service.
//! Surface-specific parsing happens here; limit/cursor policy stays in the service.

use axum::extract::{Extension, Path, State};
use axum::routing::post;
use axum::{Json, Router};
use notegate_model::Caller;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::rest::dto::{NodeOut, Page, attribution_ids, parse_kind};
use crate::state::AppState;

use notegate_service::search::{FindRequest, GrepRequest};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/workspaces/{workspace_id}/search/find", post(find))
        .route("/v1/workspaces/{workspace_id}/search/grep", post(grep))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct FindBody {
    q: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FindResponse {
    items: Vec<NodeOut>,
    page: Page,
}

#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{workspace_id}/search/find",
    tag = "search",
    params(("workspace_id" = Uuid, Path)),
    request_body = FindBody,
    responses((status = 200, description = "Find nodes", body = FindResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn find(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<FindBody>,
) -> Result<Json<FindResponse>, ApiError> {
    let kind = match body.kind.as_deref() {
        None => None,
        Some(value) => Some(parse_kind(value)?),
    };
    let page = state
        .search
        .find(
            caller.account_id(),
            workspace_id,
            FindRequest {
                q: body.q,
                path: body.path,
                kind,
                limit: body.limit,
                cursor: body.cursor,
            },
        )
        .await?;

    let refs = state
        .accounts
        .find_account_refs(&attribution_ids(page.items.iter()))
        .await?;
    let items = page
        .items
        .iter()
        .map(|view| NodeOut::from_view(view, &refs))
        .collect();

    Ok(Json(FindResponse {
        items,
        page: Page {
            limit: page.limit,
            returned: page.items.len() as i64,
            has_more: page.has_more,
            next_cursor: page.next_cursor,
        },
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct GrepBody {
    q: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    context: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct GrepMatchOut {
    node_id: Uuid,
    path: String,
    line_no: i64,
    line: String,
    before: Vec<String>,
    after: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct GrepResponse {
    matches: Vec<GrepMatchOut>,
    page: Page,
}

#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{workspace_id}/search/grep",
    tag = "search",
    params(("workspace_id" = Uuid, Path)),
    request_body = GrepBody,
    responses((status = 200, description = "Grep content", body = GrepResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn grep(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<GrepBody>,
) -> Result<Json<GrepResponse>, ApiError> {
    let page = state
        .search
        .grep(
            caller.account_id(),
            workspace_id,
            GrepRequest {
                q: body.q,
                path: body.path,
                context: body.context,
                limit: body.limit,
                cursor: body.cursor,
            },
        )
        .await?;

    let matches: Vec<GrepMatchOut> = page
        .items
        .into_iter()
        .map(|m| GrepMatchOut {
            node_id: m.node_id,
            path: m.path,
            line_no: m.line_no,
            line: m.line,
            before: m.before,
            after: m.after,
        })
        .collect();

    let returned = matches.len() as i64;
    Ok(Json(GrepResponse {
        matches,
        page: Page {
            limit: page.limit,
            returned,
            has_more: page.has_more,
            next_cursor: page.next_cursor,
        },
    }))
}
