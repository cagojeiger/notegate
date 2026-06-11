//! File MCP tools (`docs/spec/mcp/files.md`).

use axum::http::request::Parts;
use notegate_model::{NodeKind, TextStorageFormat};
use notegate_service::ServiceError;
use notegate_service::files::{
    ChildrenRequest, CreateFolder, CreateText, DeleteNode, Edit as ServiceEdit, MoveNode,
    PatchText, ReadText, ReadTextBody, WriteTarget, WriteText, WriteTextBody,
};
use notegate_service::search::TreeRequest;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::resolve::{
    caller, invalid_input_error, node_summary, resolve_target, service_error, split_parent_name,
};
use super::support::page_json;
use crate::state::AppState;

/// `files_ls` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct LsInput {
    /// Compact `<space>:/<folder-path>` target.
    pub target: String,
    /// Page size; clamped to `1..=200`, default `100`.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// `files_tree` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TreeInput {
    /// Compact `<space>:/<folder-path>` target.
    pub target: String,
    /// Maximum depth below the selected folder. Defaults to 2, max path depth.
    #[serde(default)]
    pub depth: Option<i64>,
    /// Page size; clamped to `1..=200`, default `100`.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Opaque pagination cursor from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// `files_stat` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct StatInput {
    /// Compact `<space>:/<path>` target.
    pub target: String,
}

/// `files_mkdir` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MkdirInput {
    /// Compact `<space>:/<folder-path>` target.
    pub target: String,
    /// Create missing parent folders, like `mkdir -p`.
    #[serde(default)]
    pub parents: bool,
}

/// `files_touch` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TouchInput {
    /// Compact `<space>:/<text-path>` target.
    pub target: String,
}

/// `files_read` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReadInput {
    /// Compact `<space>:/<text-path>` target.
    pub target: String,
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

/// `files_write` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WriteInput {
    /// Compact `<space>:/<text-path>` target.
    pub target: String,
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

/// `files_patch` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PatchInput {
    /// Compact `<space>:/<text-path>` target.
    pub target: String,
    /// Non-empty list of exact replacements applied against the original content.
    pub edits: Vec<PatchEdit>,
    /// Optimistic guard; checked before matching.
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

/// `files_mv` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MvInput {
    /// Compact `<space>:/<source-path>` target.
    pub source: String,
    /// Compact `<space>:/<destination-path>` target. Must be in the same space as `source`.
    pub destination: String,
}

/// `files_rm` input.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RmInput {
    /// Compact `<space>:/<path>` target.
    pub target: String,
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
    let (resolved, path) = resolve_target(state, caller, &input.target).await?;
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

pub async fn tree(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<TreeInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &input.target).await?;

    let page = state
        .search
        .tree(
            caller.account_id(),
            resolved.space_id(),
            TreeRequest {
                path: Some(path.clone()),
                depth: input.depth,
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
        "path": path,
        "depth": page.depth,
        "items": items,
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
    let (resolved, path) = resolve_target(state, caller, &input.target).await?;

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
    let (resolved, path) = resolve_target(state, caller, &input.target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    if input.parents {
        return mkdir_parents(state, account_id, space_id, resolved.name(), &path).await;
    }

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

async fn mkdir_parents(
    state: &AppState,
    account_id: uuid::Uuid,
    space_id: uuid::Uuid,
    space_name: &str,
    path: &str,
) -> Result<Json<Value>, ErrorData> {
    let mut current = state
        .files
        .resolve_path(account_id, space_id, "/")
        .await
        .map_err(service_error)?;
    let mut current_path = "/".to_owned();
    let mut created_paths = Vec::new();

    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        let next_path = if current_path == "/" {
            format!("/{segment}")
        } else {
            format!("{current_path}/{segment}")
        };

        match state
            .files
            .resolve_path(account_id, space_id, &next_path)
            .await
        {
            Ok(existing) if existing.node.kind == NodeKind::Folder => {
                current = existing;
                current_path = next_path;
            }
            Ok(_existing) => {
                return Err(service_error(ServiceError::Conflict(format!(
                    "path component '{next_path}' exists and is not a folder"
                ))));
            }
            Err(ServiceError::NotFound(_)) => {
                let created = state
                    .files
                    .create_folder(
                        account_id,
                        space_id,
                        CreateFolder {
                            parent_node_id: current.node.id,
                            name: segment.to_owned(),
                        },
                    )
                    .await
                    .map_err(service_error)?;
                created_paths.push(created.path.clone());
                current = created;
                current_path = next_path;
            }
            Err(error) => return Err(service_error(error)),
        }
    }

    Ok(Json(json!({
        "space": space_name,
        "node": node_summary(&current),
        "created_paths": created_paths,
    })))
}

pub async fn touch(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<TouchInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &input.target).await?;
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
    let (resolved, path) = resolve_target(state, caller, &input.target).await?;
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

    if result.storage_format == TextStorageFormat::Encrypted {
        return Err(service_error(ServiceError::InvalidInput(
            "encrypted text is not readable through MCP".to_owned(),
        )));
    }

    let space = resolved.name();
    let body = match &result.body {
        ReadTextBody::Unchanged => json!({
            "space": space,
            "path": result.node.path,
            "unchanged": true,
            "content_returned": false,
            "content_sha256": result.content_sha256,
        }),
        ReadTextBody::Encrypted(_) => {
            return Err(service_error(ServiceError::InvalidInput(
                "encrypted text is not readable through MCP".to_owned(),
            )));
        }
        ReadTextBody::Content(content) => json!({
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
    let (resolved, path) = resolve_target(state, caller, &input.target).await?;
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
                body: WriteTextBody::Plain(input.content),
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
    let (resolved, path) = resolve_target(state, caller, &input.target).await?;
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
    let (source_space, source_path) = resolve_target(state, caller, &input.source).await?;
    let (destination_space, destination_path) =
        resolve_target(state, caller, &input.destination).await?;
    let account_id = caller.account_id();
    let space_id = source_space.space_id();

    if destination_space.space_id() != space_id {
        return Err(invalid_input_error(
            "source and destination must be in the same space",
        ));
    }

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
        "space": source_space.name(),
        "node": node_summary(&view),
    })))
}

pub async fn rm(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<RmInput>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &input.target).await?;
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
        "deleted": true,
        "purge_after": result.purge_after,
    })))
}
