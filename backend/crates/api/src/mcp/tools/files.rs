//! File MCP tools (`docs/spec/mcp/files.md`).

use axum::http::request::Parts;
use notegate_core::validation::normalize_path;
use notegate_service::ServiceError;
use notegate_service::files::{
    ChildrenRequest, CreateDocument, CreateFolder, DeleteNode, Edit as ServiceEdit, MoveNode,
    PatchDocument, ReadDocument, WriteDocument, WriteTarget,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::resolve::{
    WorkspaceSelector, caller, invalid_input_error, node_summary, resolve_target,
    resolve_workspace, service_error, split_parent_name,
};
use super::support::page_json;
use crate::state::AppState;

/// `files_ls` input: a workspace selector plus the folder `path` (or `target`).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct LsInput {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute folder path inside the selected workspace.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Page size; clamped to `1..=200`, default `100`.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// `files_stat` input: a workspace selector plus a `path` or a `target` string.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct StatInput {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path inside the selected workspace.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
}

/// `files_mkdir` input: a workspace selector plus the folder `path` (or `target`).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct MkdirInput {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the folder to create.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
}

/// `files_touch` input: a workspace selector plus the document `path` (or `target`).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TouchInput {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the `.md` document to create.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
}

/// `files_read` input: a workspace selector, the document `path` (or `target`),
/// and optional range/conditional-read fields.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ReadInput {
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

/// `files_write` input: a workspace selector, the document `path` (or `target`),
/// the new content, and create/optimistic-guard flags.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct WriteInput {
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

/// One exact replacement.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PatchEdit {
    /// The exact text to find (must match exactly once).
    pub old_text: String,
    /// The replacement text (must differ from `old_text`).
    pub new_text: String,
}

/// `files_patch` input: a workspace selector, the document `path` (or `target`),
/// the edits, and an optional optimistic guard.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PatchInput {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the document to patch.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Non-empty list of exact replacements applied against the original content.
    pub edits: Vec<PatchEdit>,
    /// Optimistic guard; checked before matching.
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

/// `files_mv` input: a workspace selector plus source and destination paths.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct MvInput {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the node to move.
    pub source_path: String,
    /// Absolute destination path (its dirname must be an existing folder).
    pub destination_path: String,
}

/// `files_rm` input: a workspace selector, the `path` (or `target`), and the
/// recursive flag.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RmInput {
    #[serde(flatten)]
    pub selector: WorkspaceSelector,
    /// Absolute path of the node to delete.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<workspace>:/<path>` target (alternative to workspace+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Required to delete a folder (and its subtree).
    #[serde(default)]
    pub recursive: bool,
}

pub async fn ls(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<LsInput>,
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

    let folder = state
        .files
        .resolve_path(account_id, workspace_id, &path)
        .await
        .map_err(service_error)?;

    let page = state
        .files
        .children(
            account_id,
            workspace_id,
            folder.node.id,
            ChildrenRequest {
                limit: input.limit,
                cursor: input.cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let children: Vec<Value> = page.items.iter().map(node_summary).collect();
    let returned = children.len();

    Ok(Json(json!({
        "workspace": resolved.name(),
        "path": page.parent.path,
        "children": children,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    })))
}

pub async fn stat(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<StatInput>,
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

    let view = state
        .files
        .resolve_path(caller.account_id(), resolved.workspace_id(), &path)
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "workspace": resolved.name(),
        "node": node_summary(&view),
    })))
}

pub async fn mkdir(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<MkdirInput>,
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

    let (parent_path, name) = split_parent_name(&path)?;
    let parent = state
        .files
        .resolve_path(account_id, workspace_id, &parent_path)
        .await
        .map_err(service_error)?;

    let view = state
        .files
        .create_folder(
            account_id,
            workspace_id,
            CreateFolder {
                parent_node_id: parent.node.id,
                name,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "workspace": resolved.name(),
        "node": node_summary(&view),
    })))
}

pub async fn touch(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<TouchInput>,
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

    let (parent_path, name) = split_parent_name(&path)?;
    let parent = state
        .files
        .resolve_path(account_id, workspace_id, &parent_path)
        .await
        .map_err(service_error)?;

    let view = state
        .files
        .create_document(
            account_id,
            workspace_id,
            CreateDocument {
                parent_node_id: parent.node.id,
                name,
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

pub async fn read(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<ReadInput>,
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

pub async fn write(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<WriteInput>,
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
                return Err(service_error(ServiceError::NotFound(
                    "document does not exist; pass create=true to create it".to_owned(),
                )));
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

pub async fn patch(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<PatchInput>,
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

pub async fn mv(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<MvInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let resolved = resolve_workspace(state, caller, &input.selector).await?;
    let account_id = caller.account_id();
    let workspace_id = resolved.workspace_id();

    let source_path = normalize_path(&input.source_path)
        .map_err(|error| invalid_input_error(error.to_string()))?;
    let destination_path = normalize_path(&input.destination_path)
        .map_err(|error| invalid_input_error(error.to_string()))?;

    let source = state
        .files
        .resolve_path(account_id, workspace_id, &source_path)
        .await
        .map_err(service_error)?;

    let (dest_parent_path, new_name) = split_parent_name(&destination_path)?;
    let dest_parent = state
        .files
        .resolve_path(account_id, workspace_id, &dest_parent_path)
        .await
        .map_err(service_error)?;

    let view = state
        .files
        .move_node(
            account_id,
            workspace_id,
            MoveNode {
                node_id: source.node.id,
                new_parent_node_id: dest_parent.node.id,
                new_name: Some(new_name),
                expected_parent_id: None,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "workspace": resolved.name(),
        "node": node_summary(&view),
    })))
}

pub async fn rm(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<RmInput>,
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
        .delete_node(
            account_id,
            workspace_id,
            DeleteNode {
                node_id: node.node.id,
                recursive: input.recursive,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "workspace": resolved.name(),
        "path": result.path,
        "node_id": result.node_id,
        "deleted": true,
        "purge_after": result.purge_after,
    })))
}
