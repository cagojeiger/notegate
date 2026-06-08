//! `files_patch`: apply exact targeted replacements to one Markdown document
//! (`docs/spec/mcp/files.md`). Requires `editor`.
//!
//! Path-first: the path is resolved to the document node, then the edits are
//! applied by the files service, which enforces exact (non-fuzzy) matching,
//! single-match-per-edit, no-op rejection, overlap rejection, atomicity, the
//! `expected_sha256` guard, and the resulting-size limits.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use notegate_service::files::{Edit as ServiceEdit, PatchDocument};

use super::resolve::{WorkspaceSelector, caller, resolve_target, service_error};
use crate::state::AppState;

/// One exact replacement.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Edit {
    /// The exact text to find (must match exactly once).
    pub old_text: String,
    /// The replacement text (must differ from `old_text`).
    pub new_text: String,
}

/// `files_patch` input: a workspace selector, the document `path` (or `target`),
/// the edits, and an optional optimistic guard.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the document to patch.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Non-empty list of exact replacements applied against the original content.
    pub edits: Vec<Edit>,
    /// Optimistic guard; checked before matching.
    #[serde(default)]
    pub expected_sha256: Option<String>,
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

    let edits = input
        .edits
        .into_iter()
        .map(|edit| ServiceEdit {
            old_text: edit.old_text,
            new_text: edit.new_text,
        })
        .collect();

    let result = state
        .files
        .patch_document(
            account_id,
            workspace_id,
            PatchDocument {
                node_id: node.node.id,
                edits,
                expected_sha256: input.expected_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "workspace": resolved.name(),
        "path": result.node.path,
        "patched": true,
        "edits_applied": result.edits_applied,
        "content_sha256": result.document.content_sha256,
        "previous_sha256": result.previous_sha256,
        "byte_len": result.document.byte_len,
        "line_count": result.document.line_count,
        "diff": result.diff,
    })))
}
