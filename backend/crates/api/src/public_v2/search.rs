use axum::extract::{Extension, Path, State};
use axum::routing::post;
use axum::{Json, Router};
use notegate_model::{Caller, NodeKind};
use notegate_service::search::{
    FindMatchMode, FindRequest, GrepLineMode, GrepMatchMode, GrepRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

use super::dto::{NodeSummaryOut, PageOut};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/spaces/{space_id}/search/find", post(find))
        .route("/spaces/{space_id}/search/grep", post(grep))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Finds nodes by name or canonical path inside a bounded folder scope.
#[schema(example = json!({
    "q": "README",
    "path": "/docs",
    "kind": "text",
    "match": "contains",
    "include": ["**/*.md"],
    "exclude": ["archive/**"],
    "limit": 50
}))]
pub(crate) struct FindBody {
    /// Single-line search query.
    q: String,
    /// Absolute folder path used as the search scope. Defaults to `/`.
    #[schema(default = "/", example = "/docs")]
    #[serde(default = "default_path")]
    path: String,
    /// Optional node kind filter: `folder`, `text`, or `file`.
    #[schema(examples("folder", "text", "file"))]
    #[serde(default)]
    kind: Option<String>,
    /// Matching strategy: `contains` (default), `regex`, or `glob`.
    #[schema(default = "contains", examples("contains", "regex", "glob"))]
    #[serde(default, rename = "match")]
    match_mode: Option<String>,
    /// Glob patterns that a canonical relative path must match.
    #[serde(default)]
    include: Vec<String>,
    /// Glob patterns excluded after include matching.
    #[serde(default)]
    exclude: Vec<String>,
    /// Page size. Defaults to 50 and is capped at 100.
    #[schema(default = 50, minimum = 1, maximum = 100)]
    #[serde(default)]
    limit: Option<i64>,
    /// Opaque continuation cursor returned by the preceding response.
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SearchResponse {
    items: Vec<SearchHitOut>,
    page: PageOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SearchHitOut {
    #[serde(flatten)]
    node: NodeSummaryOut,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    match_lines: Vec<i32>,
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/search/find",
    tag = "search",
    params(("space_id" = Uuid, Path)),
    request_body = FindBody,
    responses(
        (status = 200, description = "Find nodes by name or path", body = SearchResponse),
        (status = 429, description = "Find capacity is busy", body = crate::error::ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub(crate) async fn find(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Json(body): Json<FindBody>,
) -> Result<Json<SearchResponse>, ApiError> {
    let _permit = state
        .search_admission
        .enter_find()
        .map_err(|_| ApiError::search_busy("find"))?;
    let kind = body.kind.as_deref().map(parse_kind).transpose()?;
    let page = state
        .search
        .find(
            caller.account_id(),
            space_id,
            FindRequest {
                q: body.q,
                path: Some(body.path),
                kind,
                match_mode: parse_find_match_mode(body.match_mode.as_deref())?,
                include: body.include,
                exclude: body.exclude,
                limit: body.limit,
                cursor: body.cursor,
            },
        )
        .await?;
    let items = page
        .items
        .iter()
        .map(|view| SearchHitOut {
            node: NodeSummaryOut::from_view(view),
            match_lines: Vec::new(),
        })
        .collect::<Vec<_>>();
    Ok(Json(SearchResponse {
        page: PageOut::new(page.limit, items.len(), page.has_more, page.next_cursor),
        items,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Searches plain-text content inside a bounded folder scope.
#[schema(example = json!({
    "q": "TODO",
    "path": "/docs",
    "match": "literal",
    "lines": "first",
    "include": ["**/*.md"],
    "limit": 20
}))]
pub(crate) struct GrepBody {
    /// Single-line content query.
    q: String,
    /// Absolute folder path used as the search scope. Defaults to `/`.
    #[schema(default = "/", example = "/docs")]
    #[serde(default = "default_path")]
    path: String,
    /// Matching strategy: `literal` (default) or `regex`.
    #[schema(default = "literal", examples("literal", "regex"))]
    #[serde(default, rename = "match")]
    match_mode: Option<String>,
    /// Matching line details: `none` (default), `first`, or `all`.
    #[schema(default = "none", examples("none", "first", "all"))]
    #[serde(default)]
    lines: Option<String>,
    /// Glob patterns that a canonical relative path must match.
    #[serde(default)]
    include: Vec<String>,
    /// Glob patterns excluded after include matching.
    #[serde(default)]
    exclude: Vec<String>,
    /// Page size. Defaults to 20 and is capped at 100.
    #[schema(default = 20, minimum = 1, maximum = 100)]
    #[serde(default)]
    limit: Option<i64>,
    /// Opaque continuation cursor returned by the preceding response.
    #[serde(default)]
    cursor: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/search/grep",
    tag = "search",
    params(("space_id" = Uuid, Path)),
    request_body = GrepBody,
    responses(
        (status = 200, description = "Search plain-text content", body = SearchResponse),
        (status = 429, description = "Grep capacity is busy", body = crate::error::ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub(crate) async fn grep(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Json(body): Json<GrepBody>,
) -> Result<Json<SearchResponse>, ApiError> {
    let _permit = state
        .search_admission
        .enter_grep()
        .await
        .map_err(|_| ApiError::search_busy("grep"))?;
    let page = state
        .search
        .grep(
            caller.account_id(),
            space_id,
            GrepRequest {
                q: body.q,
                path: Some(body.path),
                match_mode: parse_grep_match_mode(body.match_mode.as_deref())?,
                line_mode: parse_grep_line_mode(body.lines.as_deref())?,
                include: body.include,
                exclude: body.exclude,
                limit: body.limit,
                cursor: body.cursor,
            },
        )
        .await?;
    let items = page
        .items
        .iter()
        .map(|hit| SearchHitOut {
            node: NodeSummaryOut::from_view(&hit.node),
            match_lines: hit.match_lines.clone(),
        })
        .collect::<Vec<_>>();
    Ok(Json(SearchResponse {
        page: PageOut::new(page.limit, items.len(), page.has_more, page.next_cursor),
        items,
    }))
}

fn default_path() -> String {
    "/".to_owned()
}

fn parse_kind(value: &str) -> Result<NodeKind, ApiError> {
    NodeKind::parse(value)
        .ok_or_else(|| ApiError::invalid_field("kind must be 'folder', 'text', or 'file'"))
}

fn parse_find_match_mode(value: Option<&str>) -> Result<FindMatchMode, ApiError> {
    FindMatchMode::parse(value.unwrap_or("contains"))
        .ok_or_else(|| ApiError::invalid_field("match must be 'contains', 'regex', or 'glob'"))
}

fn parse_grep_match_mode(value: Option<&str>) -> Result<GrepMatchMode, ApiError> {
    GrepMatchMode::parse(value.unwrap_or("literal"))
        .ok_or_else(|| ApiError::invalid_field("match must be 'literal' or 'regex'"))
}

fn parse_grep_line_mode(value: Option<&str>) -> Result<GrepLineMode, ApiError> {
    GrepLineMode::parse(value.unwrap_or("none"))
        .ok_or_else(|| ApiError::invalid_field("lines must be 'none', 'first', or 'all'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_defaults_match_agent_mcp_contract() {
        assert_eq!(
            parse_find_match_mode(None).ok(),
            Some(FindMatchMode::Contains)
        );
        assert_eq!(
            parse_grep_match_mode(None).ok(),
            Some(GrepMatchMode::Literal)
        );
        assert_eq!(parse_grep_line_mode(None).ok(), Some(GrepLineMode::None));
    }

    #[test]
    fn search_rejects_unknown_modes() {
        assert!(parse_find_match_mode(Some("literal")).is_err());
        assert!(parse_grep_match_mode(Some("glob")).is_err());
        assert!(parse_grep_line_mode(Some("matching")).is_err());
    }
}
