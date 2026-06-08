//! `files_read`: read a Markdown document with range limits (mcp-tools.md).
//!
//! Resolves the path to a document node, then reads a bounded line/byte slice.
//! When `if_none_match_sha256` matches the current content hash, the tool returns
//! the `unchanged` shape without content.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use notegate_service::files::ReadDocument;

use super::resolve::{WorkspaceSelector, caller, resolve_target, service_error};
use crate::state::AppState;

/// `files_read` input: a workspace selector, the document `path` (or `target`),
/// and optional range/conditional-read fields.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the document to read.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
    /// 1-based first line to return; defaults to `1`.
    #[serde(default)]
    pub start_line: Option<i64>,
    /// Maximum lines to return; clamped to the read limit.
    #[serde(default)]
    pub max_lines: Option<i64>,
    /// Maximum bytes to return; clamped to the read limit.
    #[serde(default)]
    pub max_bytes: Option<usize>,
    /// Return the `unchanged` shape when this equals the current content hash.
    #[serde(default)]
    pub if_none_match_sha256: Option<String>,
}

pub async fn call(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<Input>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(
        state,
        caller,
        &input.selector,
        input.target.as_deref(),
        input.path.as_deref(),
    )
    .await?;
    let account_id = caller.account_id();
    let workspace_id = resolved.workspace_id();

    let node = state
        .files
        .resolve_path(account_id, workspace_id, &path)
        .await
        .map_err(service_error)?;

    let result = state
        .files
        .read_document(
            account_id,
            workspace_id,
            ReadDocument {
                node_id: node.node.id,
                start_line: input.start_line,
                max_lines: input.max_lines,
                max_bytes: input.max_bytes,
                if_none_match_sha256: input.if_none_match_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    let workspace = resolved.name();
    let body = match &result.content {
        // Conditional-read hit: metadata only, no content.
        None => json!({
            "workspace": workspace,
            "path": result.node.path,
            "unchanged": true,
            "content_returned": false,
            "content_sha256": result.content_sha256,
        }),
        Some(content) => json!({
            "workspace": workspace,
            "path": result.node.path,
            "content_md": content.content_md,
            "content_sha256": result.content_sha256,
            "byte_len": result.byte_len,
            "line_count": result.line_count,
            "start_line": content.start_line,
            "end_line": content.end_line,
            "returned_lines": content.returned_lines,
            "truncated": content.truncated,
            "next_start_line": content.next_start_line,
        }),
    };
    Ok(Json(body))
}
