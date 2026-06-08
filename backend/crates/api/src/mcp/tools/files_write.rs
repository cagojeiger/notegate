//! `files_write`: replace (or optionally create) a Markdown document
//! (`docs/spec/mcp/files.md`). Requires `editor`.
//!
//! Path-first: the path is resolved to decide the write target. A resolved
//! document is replaced (`WriteTarget::Existing`). A missing path with
//! `create=true` is created under its dirname (`WriteTarget::Create`); a missing
//! path with `create=false` is a not-found error returned before the service is
//! invoked. Size/quota/`expected_sha256` checks live in the files service.

use axum::http::request::Parts;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use notegate_service::ServiceError;
use notegate_service::files::{WriteDocument, WriteTarget};

use super::resolve::{
    WorkspaceSelector, caller, invalid_input_error, node_summary, resolve_target, service_error,
    split_parent_name,
};
use crate::state::AppState;

/// `files_write` input: a workspace selector, the document `path` (or `target`),
/// the new content, and create/optimistic-guard flags.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Input {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the document to write.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
    /// The full replacement Markdown content.
    pub content_md: String,
    /// When true, a missing document is created; when false, it must exist.
    #[serde(default)]
    pub create: bool,
    /// Optimistic guard; conflict if it does not match the current content hash.
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

    // Resolve the path to choose the write target. NotFound is expected for a
    // create; any other error propagates.
    let existing = match state
        .files
        .resolve_path(account_id, workspace_id, &path)
        .await
    {
        Ok(view) => Some(view),
        Err(ServiceError::NotFound(_)) => None,
        Err(error) => return Err(service_error(error)),
    };

    let target = match existing {
        Some(view) => WriteTarget::Existing {
            node_id: view.node.id,
        },
        None => {
            if !input.create {
                return Err(invalid_input_error(
                    "document does not exist; pass create=true to create it",
                ));
            }
            let (parent_path, name) = split_parent_name(&path)?;
            let parent = state
                .files
                .resolve_path(account_id, workspace_id, &parent_path)
                .await
                .map_err(service_error)?;
            WriteTarget::Create {
                parent_node_id: parent.node.id,
                name,
            }
        }
    };

    let view = state
        .files
        .write_document(
            account_id,
            workspace_id,
            WriteDocument {
                target,
                content_md: input.content_md,
                expected_sha256: input.expected_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "workspace": resolved.name(),
        "node": node_summary(&view.node),
        "content_sha256": view.document.content_sha256,
        "byte_len": view.document.byte_len,
        "line_count": view.document.line_count,
    })))
}
