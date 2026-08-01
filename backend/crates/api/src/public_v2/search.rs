use axum::extract::{Extension, Path, State};
use axum::routing::post;
use axum::{Json, Router};
use notegate_model::{Caller, NodeKind};
use notegate_service::search::{
    FindMatchMode, FindRequest, GrepLineMode, GrepMatchMode, GrepRequest,
};
use serde::{Deserialize, Serialize};
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
    #[serde(default)]
    kind: Option<SearchNodeKind>,
    /// Matching strategy: `contains` (default), `regex`, or `glob`.
    #[schema(default = "contains")]
    #[serde(default, rename = "match")]
    match_mode: FindMatch,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SearchNodeKind {
    Folder,
    Text,
    File,
}

impl From<SearchNodeKind> for NodeKind {
    fn from(value: SearchNodeKind) -> Self {
        match value {
            SearchNodeKind::Folder => Self::Folder,
            SearchNodeKind::Text => Self::Text,
            SearchNodeKind::File => Self::File,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FindMatch {
    #[default]
    Contains,
    Regex,
    Glob,
}

impl From<FindMatch> for FindMatchMode {
    fn from(value: FindMatch) -> Self {
        match value {
            FindMatch::Contains => Self::Contains,
            FindMatch::Regex => Self::Regex,
            FindMatch::Glob => Self::Glob,
        }
    }
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
    operation_id = "find_nodes",
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
    let page = state
        .search
        .find(
            caller.account_id(),
            space_id,
            FindRequest {
                q: body.q,
                path: Some(body.path),
                kind: body.kind.map(NodeKind::from),
                match_mode: body.match_mode.into(),
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
    #[schema(default = "literal")]
    #[serde(default, rename = "match")]
    match_mode: GrepMatch,
    /// Matching line details: `none` (default), `first`, or `all`.
    #[schema(default = "none")]
    #[serde(default)]
    lines: GrepLines,
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GrepMatch {
    #[default]
    Literal,
    Regex,
}

impl From<GrepMatch> for GrepMatchMode {
    fn from(value: GrepMatch) -> Self {
        match value {
            GrepMatch::Literal => Self::Literal,
            GrepMatch::Regex => Self::Regex,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GrepLines {
    #[default]
    None,
    First,
    All,
}

impl From<GrepLines> for GrepLineMode {
    fn from(value: GrepLines) -> Self {
        match value {
            GrepLines::None => Self::None,
            GrepLines::First => Self::First,
            GrepLines::All => Self::All,
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/search/grep",
    operation_id = "grep_text",
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
                match_mode: body.match_mode.into(),
                line_mode: body.lines.into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_defaults_match_agent_mcp_contract() -> Result<(), serde_json::Error> {
        let find: FindBody = serde_json::from_value(serde_json::json!({"q": "README"}))?;
        assert_eq!(find.match_mode, FindMatch::Contains);

        let grep: GrepBody = serde_json::from_value(serde_json::json!({"q": "TODO"}))?;
        assert_eq!(grep.match_mode, GrepMatch::Literal);
        assert_eq!(grep.lines, GrepLines::None);
        Ok(())
    }

    #[test]
    fn search_rejects_unknown_modes() {
        assert!(
            serde_json::from_value::<FindBody>(
                serde_json::json!({"q": "README", "match": "literal"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<GrepBody>(serde_json::json!({"q": "TODO", "match": "glob"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<GrepBody>(
                serde_json::json!({"q": "TODO", "lines": "matching"})
            )
            .is_err()
        );
    }
}
