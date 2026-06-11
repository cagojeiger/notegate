//! File MCP tools (`docs/spec/mcp/files.md`).

use axum::http::request::Parts;
use notegate_core::validation::normalize_path;
use notegate_service::ServiceError;
use notegate_service::files::{
    ChildrenRequest, CreateFolder, CreateText, DeleteNode, Edit as ServiceEdit, MoveNode,
    PatchText, ReadText, WriteTarget, WriteText,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::resolve::{
    SpaceSelector, caller, invalid_input_error, node_summary, resolve_space, resolve_target,
    service_error, split_parent_name,
};
use super::support::page_json;
use crate::state::AppState;

/// `files_ls` input: a space selector plus the folder `path` (or `target`).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct LsInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// Absolute folder path inside the selected space.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<space>:/<path>` target (alternative to space+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Page size; clamped to `1..=200`, default `100`.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// `files_stat` input: a space selector plus a `path` or a `target` string.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct StatInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// Absolute path inside the selected space.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<space>:/<path>` target (alternative to space+path).
    #[serde(default)]
    pub target: Option<String>,
}

/// `files_mkdir` input: a space selector plus the folder `path` (or `target`).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct MkdirInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// Absolute path of the folder to create.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<space>:/<path>` target (alternative to space+path).
    #[serde(default)]
    pub target: Option<String>,
}

/// `files_touch` input: a space selector plus the text `path` (or `target`).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TouchInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// Absolute path of the text node to create.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<space>:/<path>` target (alternative to space+path).
    #[serde(default)]
    pub target: Option<String>,
}

/// `files_read` input: a space selector, the text `path` (or `target`),
/// and optional range/conditional-read fields.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ReadInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// Absolute path of the text to read.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<space>:/<path>` target (alternative to space+path).
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

/// `files_write` input: a space selector, the text `path` (or `target`),
/// the new content, and create/optimistic-guard flags.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct WriteInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// Absolute path of the text to write.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<space>:/<path>` target (alternative to space+path).
    #[serde(default)]
    pub target: Option<String>,
    /// The full replacement text content.
    pub content: String,
    /// When true, a missing text is created; when false, it must exist.
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

/// `files_patch` input: a space selector, the text `path` (or `target`),
/// the edits, and an optional optimistic guard.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PatchInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// Absolute path of the text to patch.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<space>:/<path>` target (alternative to space+path).
    #[serde(default)]
    pub target: Option<String>,
    /// Non-empty list of exact replacements applied against the original content.
    pub edits: Vec<PatchEdit>,
    /// Optimistic guard; checked before matching.
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

/// `files_mv` input: a space selector plus source and destination paths.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct MvInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// Absolute path of the node to move.
    pub source_path: String,
    /// Absolute destination path (its dirname must be an existing folder).
    pub destination_path: String,
}

/// `files_rm` input: a space selector, the `path` (or `target`), and the
/// recursive flag.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RmInput {
    #[serde(flatten)]
    pub selector: SpaceSelector,
    /// Absolute path of the node to delete.
    #[serde(default)]
    pub path: Option<String>,
    /// Compact `<space>:/<path>` target (alternative to space+path).
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
    let space_id = resolved.space_id();

    let folder = state
        .files
        .resolve_path(account_id, space_id, &path)
        .await
        .map_err(service_error)?;

    let page = state
        .files
        .children(
            account_id,
            space_id,
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
        "space": resolved.name(),
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
        .resolve_path(caller.account_id(), resolved.space_id(), &path)
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
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
    let space_id = resolved.space_id();

    let (parent_path, name) = split_parent_name(&path)?;
    let parent = state
        .files
        .resolve_path(account_id, space_id, &parent_path)
        .await
        .map_err(service_error)?;

    let view = state
        .files
        .create_folder(
            account_id,
            space_id,
            CreateFolder {
                parent_node_id: parent.node.id,
                name,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
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
    let space_id = resolved.space_id();

    let (parent_path, name) = split_parent_name(&path)?;
    let parent = state
        .files
        .resolve_path(account_id, space_id, &parent_path)
        .await
        .map_err(service_error)?;

    let view = state
        .files
        .create_text(
            account_id,
            space_id,
            CreateText {
                parent_node_id: parent.node.id,
                name,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "node": node_summary(&view.node),
        "content_sha256": view.text.content_sha256,
        "byte_len": view.text.byte_len,
        "line_count": view.text.line_count,
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
    let space_id = resolved.space_id();

    let node = state
        .files
        .resolve_path(account_id, space_id, &path)
        .await
        .map_err(service_error)?;

    let result = state
        .files
        .read_text(
            account_id,
            space_id,
            ReadText {
                node_id: node.node.id,
                start_line: input.start_line,
                max_lines: input.max_lines,
                max_bytes: input.max_bytes,
                if_none_match_sha256: input.if_none_match_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    let space = resolved.name();
    let body = match &result.content {
        None => json!({
            "space": space,
            "path": result.node.path,
            "unchanged": true,
            "content_returned": false,
            "content_sha256": result.content_sha256,
        }),
        Some(content) => json!({
            "space": space,
            "path": result.node.path,
            "content": content.content,
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
    let space_id = resolved.space_id();

    let existing = match state.files.resolve_path(account_id, space_id, &path).await {
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
                    "text does not exist; pass create=true to create it".to_owned(),
                )));
            }
            let (parent_path, name) = split_parent_name(&path)?;
            let parent = state
                .files
                .resolve_path(account_id, space_id, &parent_path)
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
        .write_text(
            account_id,
            space_id,
            WriteText {
                target,
                content: input.content,
                expected_sha256: input.expected_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "node": node_summary(&view.node),
        "content_sha256": view.text.content_sha256,
        "byte_len": view.text.byte_len,
        "line_count": view.text.line_count,
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
    let space_id = resolved.space_id();

    let node = state
        .files
        .resolve_path(account_id, space_id, &path)
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
        .patch_text(
            account_id,
            space_id,
            PatchText {
                node_id: node.node.id,
                edits,
                expected_sha256: input.expected_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "path": result.node.path,
        "patched": true,
        "edits_applied": result.edits_applied,
        "content_sha256": result.text.content_sha256,
        "previous_sha256": result.previous_sha256,
        "byte_len": result.text.byte_len,
        "line_count": result.text.line_count,
        "diff": result.diff,
    })))
}

pub async fn mv(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<MvInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let resolved = resolve_space(state, caller, &input.selector).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let source_path = normalize_path(&input.source_path)
        .map_err(|error| invalid_input_error(error.to_string()))?;
    let destination_path = normalize_path(&input.destination_path)
        .map_err(|error| invalid_input_error(error.to_string()))?;

    let source = state
        .files
        .resolve_path(account_id, space_id, &source_path)
        .await
        .map_err(service_error)?;

    let (dest_parent_path, new_name) = split_parent_name(&destination_path)?;
    let dest_parent = state
        .files
        .resolve_path(account_id, space_id, &dest_parent_path)
        .await
        .map_err(service_error)?;

    let view = state
        .files
        .move_node(
            account_id,
            space_id,
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
        "space": resolved.name(),
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
    let space_id = resolved.space_id();

    let node = state
        .files
        .resolve_path(account_id, space_id, &path)
        .await
        .map_err(service_error)?;

    let result = state
        .files
        .delete_node(
            account_id,
            space_id,
            DeleteNode {
                node_id: node.node.id,
                recursive: input.recursive,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "path": result.path,
        "node_id": result.node_id,
        "deleted": true,
        "purge_after": result.purge_after,
    })))
}
