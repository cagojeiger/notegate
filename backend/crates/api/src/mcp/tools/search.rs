//! Search MCP tools (`docs/spec/mcp/search.md`).

use axum::http::request::Parts;
use notegate_model::NodeKind;
use notegate_service::search::{FindRequest, GrepRequest};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::resolve::{
    SpaceSelector, caller, invalid_input_error, node_summary, resolve_space, resolve_target,
    service_error,
};
use super::support::page_json;
use crate::state::AppState;

/// `files_find` input: a space selector, the query, and optional target/
/// scope/kind/paging fields.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct FindInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// The node-name substring to match.
    pub q: String,
    /// Optional absolute scope path to search within.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<space>:/<scope-path>` target (alternative to space+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Optional kind filter: `folder`, `text`, or `file`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Match mode: `contains`, `regex`, or `glob`.
    #[serde(default, rename = "match")]
    pub match_mode: Option<String>,
    /// Page size; clamped to the find limit, default `50`.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// `files_grep` input: a space selector, the query, and optional target/
/// scope/match/paging fields.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct GrepInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// The content query to match.
    pub q: String,
    /// Optional absolute scope path to search within.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<space>:/<scope-path>` target (alternative to space+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Match mode: `literal` or `regex`.
    #[serde(default, rename = "match")]
    pub match_mode: Option<String>,
    /// Optional include path globs.
    #[serde(default)]
    pub include: Option<Vec<String>>,
    /// Optional exclude path globs.
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
    /// Page size; clamped to the grep limit, default `20`.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
}

pub async fn find(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<FindInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, scope_path) = match input.target.as_deref() {
        Some(target) => {
            let (resolved, path) =
                resolve_target(state, caller, &input.selector, Some(target), None).await?;
            (resolved, Some(path))
        }
        None => (
            resolve_space(state, caller, &input.selector).await?,
            input.path,
        ),
    };

    let kind = match input.kind.as_deref() {
        None => None,
        Some(value) => Some(parse_kind(value)?),
    };
    let _match_mode = input.match_mode;

    let page = state
        .search
        .find(
            caller.account_id(),
            resolved.space_id(),
            FindRequest {
                q: input.q,
                path: scope_path,
                kind,
                limit: input.limit,
                cursor: input.cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let items: Vec<Value> = page.items.iter().map(node_summary).collect();
    let returned = items.len();

    Ok(Json(json!({
        "space": resolved.name(),
        "items": items,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    })))
}

pub async fn grep(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<GrepInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, scope_path) = match input.target.as_deref() {
        Some(target) => {
            let (resolved, path) =
                resolve_target(state, caller, &input.selector, Some(target), None).await?;
            (resolved, Some(path))
        }
        None => (
            resolve_space(state, caller, &input.selector).await?,
            input.path,
        ),
    };
    let space = resolved.name().to_owned();
    let _match_mode = input.match_mode;
    let _include = input.include;
    let _exclude = input.exclude;

    let page = state
        .search
        .grep(
            caller.account_id(),
            resolved.space_id(),
            GrepRequest {
                q: input.q,
                path: scope_path,
                limit: input.limit,
                cursor: input.cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let items: Vec<Value> = page.items.iter().map(node_summary).collect();
    let returned = items.len();

    Ok(Json(json!({
        "space": space,
        "items": items,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    })))
}

fn parse_kind(value: &str) -> Result<NodeKind, ErrorData> {
    NodeKind::parse(value)
        .ok_or_else(|| invalid_input_error("kind must be 'folder', 'text', or 'file'"))
}
