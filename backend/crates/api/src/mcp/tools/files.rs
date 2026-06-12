//! Internal file operation handlers used by the unified MCP tools (`docs/spec/mcp/tools.md`).

use axum::http::request::Parts;
use notegate_model::{NodeKind, TextStorageFormat};
use notegate_service::ServiceError;
use notegate_service::files::{
    AppendText, ChildrenRequest, CopyNode, CreateFolder, DeleteNode, Edit as ServiceEdit, EditText,
    LineEdit, MoveNode, PatchMode, PatchText, ReadText, ReadTextBody, WriteTarget, WriteText,
    WriteTextBody,
};
use notegate_service::search::TreeRequest;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::resolve::{
    caller, invalid_input_error, node_summary, resolve_target, service_error, split_parent_name,
};
use super::support::page_json;
use crate::state::AppState;

/// One exact replacement.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchEdit {
    /// The exact text to find (must match exactly once).
    pub old_text: String,
    /// The replacement text (must differ from `old_text`).
    pub new_text: String,
    /// Replacement mode: `unique` (default), `first`, or `all`.
    #[serde(default)]
    pub mode: Option<String>,
    /// Optional guard for the number of matches in the current text.
    #[serde(default)]
    pub expected_count: Option<usize>,
}

/// One line-based edit.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineEditInput {
    /// `insert_before_line`, `insert_after_line`, `replace_lines`, or `delete_lines`.
    pub op: String,
    /// 1-based line for insert operations.
    #[serde(default)]
    pub line: Option<i64>,
    /// 1-based first line for replace/delete operations.
    #[serde(default)]
    pub start_line: Option<i64>,
    /// 1-based last line for replace/delete operations.
    #[serde(default)]
    pub end_line: Option<i64>,
    /// Content to insert or replace with.
    #[serde(default)]
    pub content: Option<String>,
}

pub async fn list(
    state: &AppState,
    parts: &Parts,
    target: String,
    depth: Option<i64>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();
    let depth = depth.unwrap_or(1);

    if depth < 1 {
        return Err(invalid_input_error("depth must be at least 1"));
    }

    if depth == 1 {
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
                ChildrenRequest { limit, cursor },
            )
            .await
            .map_err(service_error)?;

        let items: Vec<Value> = page.items.iter().map(node_summary).collect();
        let returned = items.len();

        return Ok(Json(json!({
            "space": resolved.name(),
            "path": page.parent.path,
            "depth": 1,
            "items": items,
            "page": page_json(
                page.limit,
                returned,
                page.has_more,
                page.next_cursor.as_deref(),
            ),
        })));
    }

    let page = state
        .search
        .tree(
            account_id,
            space_id,
            TreeRequest {
                path: Some(path.clone()),
                depth: Some(depth),
                limit,
                cursor,
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
    target: String,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;

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
    target: String,
    parents: bool,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    if parents {
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

pub async fn read(
    state: &AppState,
    parts: &Parts,
    target: String,
    start_line: Option<i64>,
    max_lines: Option<i64>,
    max_bytes: Option<usize>,
    if_none_match_sha256: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
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
                start_line,
                max_lines,
                max_bytes,
                if_none_match_sha256,
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
    target: String,
    content: String,
    create: bool,
    expected_sha256: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let existing = match state.files.resolve_path(account_id, space_id, &path).await {
        Ok(view) => Some(view),
        Err(ServiceError::NotFound(_)) => None,
        Err(error) => return Err(service_error(error)),
    };

    if let Some(view) = &existing {
        ensure_mcp_plain_text(state, account_id, space_id, view.node.id).await?;
    }

    let target = match existing {
        Some(view) => WriteTarget::Existing {
            node_id: view.node.id,
        },
        None => {
            if !create {
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
                body: WriteTextBody::Plain(content),
                expected_sha256,
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

pub async fn append(
    state: &AppState,
    parts: &Parts,
    target: String,
    content: String,
    create: bool,
    ensure_newline: bool,
    expected_sha256: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
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
            if !create {
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
        .append_text(
            account_id,
            space_id,
            AppendText {
                target,
                content,
                expected_sha256,
                ensure_newline,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "node": node_summary(&view.node),
        "appended": true,
        "content_sha256": view.text.content_sha256,
        "byte_len": view.text.byte_len,
        "line_count": view.text.line_count,
    })))
}

pub async fn patch(
    state: &AppState,
    parts: &Parts,
    target: String,
    edits: Vec<PatchEdit>,
    expected_sha256: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let node = state
        .files
        .resolve_path(account_id, space_id, &path)
        .await
        .map_err(service_error)?;

    let edits = edits
        .into_iter()
        .map(|edit| {
            Ok(ServiceEdit {
                old_text: edit.old_text,
                new_text: edit.new_text,
                mode: parse_patch_mode(edit.mode.as_deref())?,
                expected_count: edit.expected_count,
            })
        })
        .collect::<Result<Vec<_>, ErrorData>>()?;

    let result = state
        .files
        .patch_text(
            account_id,
            space_id,
            PatchText {
                node_id: node.node.id,
                edits,
                expected_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "path": result.node.path,
        "node": node_summary(&result.node),
        "patched": true,
        "edits_applied": result.edits_applied,
        "content_sha256": result.text.content_sha256,
        "previous_sha256": result.previous_sha256,
        "byte_len": result.text.byte_len,
        "line_count": result.text.line_count,
        "diff": result.diff,
    })))
}

pub async fn edit(
    state: &AppState,
    parts: &Parts,
    target: String,
    edits: Vec<LineEditInput>,
    expected_sha256: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let node = state
        .files
        .resolve_path(account_id, space_id, &path)
        .await
        .map_err(service_error)?;

    let edits = edits
        .into_iter()
        .map(parse_line_edit)
        .collect::<Result<Vec<_>, ErrorData>>()?;

    let result = state
        .files
        .edit_text(
            account_id,
            space_id,
            EditText {
                node_id: node.node.id,
                edits,
                expected_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "path": result.node.path,
        "node": node_summary(&result.node),
        "edited": true,
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
    source: String,
    destination: String,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (source_space, source_path) = resolve_target(state, caller, &source).await?;
    let (destination_space, destination_path) = resolve_target(state, caller, &destination).await?;
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

pub async fn copy(
    state: &AppState,
    parts: &Parts,
    source: String,
    destination: String,
    recursive: bool,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (source_space, source_path) = resolve_target(state, caller, &source).await?;
    let (destination_space, destination_path) = resolve_target(state, caller, &destination).await?;
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
    let (parent_path, new_name) = split_parent_name(&destination_path)?;
    let parent = state
        .files
        .resolve_path(account_id, space_id, &parent_path)
        .await
        .map_err(service_error)?;

    let result = state
        .files
        .copy_node(
            account_id,
            space_id,
            CopyNode {
                node_id: source.node.id,
                new_parent_node_id: parent.node.id,
                new_name,
                recursive,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": source_space.name(),
        "source_path": source_path,
        "node": node_summary(&result.node),
        "copied": true,
        "counts": {
            "nodes": result.counts.nodes,
            "texts": result.counts.texts,
            "files": result.counts.files,
        },
    })))
}

pub async fn rm(
    state: &AppState,
    parts: &Parts,
    target: String,
    recursive: bool,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
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
                recursive,
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

async fn ensure_mcp_plain_text(
    state: &AppState,
    account_id: uuid::Uuid,
    space_id: uuid::Uuid,
    node_id: uuid::Uuid,
) -> Result<(), ErrorData> {
    let result = state
        .files
        .read_text(
            account_id,
            space_id,
            ReadText {
                node_id,
                start_line: None,
                max_lines: None,
                max_bytes: Some(1),
                if_none_match_sha256: None,
            },
        )
        .await
        .map_err(service_error)?;
    if result.storage_format == TextStorageFormat::Encrypted {
        return Err(service_error(ServiceError::InvalidInput(
            "encrypted text cannot be modified through MCP content tools".to_owned(),
        )));
    }
    Ok(())
}

fn parse_patch_mode(raw: Option<&str>) -> Result<PatchMode, ErrorData> {
    match raw.unwrap_or("unique") {
        "unique" => Ok(PatchMode::Unique),
        "first" => Ok(PatchMode::First),
        "all" => Ok(PatchMode::All),
        _ => Err(invalid_input_error(
            "mode must be 'unique', 'first', or 'all'",
        )),
    }
}

fn parse_line_edit(input: LineEditInput) -> Result<LineEdit, ErrorData> {
    match input.op.as_str() {
        "insert_before_line" => Ok(LineEdit::InsertBefore {
            line: required_i64(input.line, "line")?,
            content: required_string(input.content, "content")?,
        }),
        "insert_after_line" => Ok(LineEdit::InsertAfter {
            line: required_i64(input.line, "line")?,
            content: required_string(input.content, "content")?,
        }),
        "replace_lines" => Ok(LineEdit::ReplaceLines {
            start_line: required_i64(input.start_line, "start_line")?,
            end_line: required_i64(input.end_line, "end_line")?,
            content: required_string(input.content, "content")?,
        }),
        "delete_lines" => Ok(LineEdit::DeleteLines {
            start_line: required_i64(input.start_line, "start_line")?,
            end_line: required_i64(input.end_line, "end_line")?,
        }),
        _ => Err(invalid_input_error(
            "op must be insert_before_line, insert_after_line, replace_lines, or delete_lines",
        )),
    }
}

fn required_i64(value: Option<i64>, field: &'static str) -> Result<i64, ErrorData> {
    value.ok_or_else(|| invalid_input_error(format!("{field} is required")))
}

fn required_string(value: Option<String>, field: &'static str) -> Result<String, ErrorData> {
    value.ok_or_else(|| invalid_input_error(format!("{field} is required")))
}
