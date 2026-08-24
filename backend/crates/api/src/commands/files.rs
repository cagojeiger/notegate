//! Protocol-neutral file command handlers shared by transport adapters.

use notegate_command::{CommandError, LineEditInput, PatchEdit};
use notegate_model::TextStorageFormat;
use notegate_service::ServiceError;
use notegate_service::files::{
    AppendText, ChildrenRequest, CopyNode, CreateFolder, DeleteNode, Edit as ServiceEdit, EditText,
    LineEdit, MoveNode, NodeView, PatchError, PatchMode, PatchText, ReadText, ReadTextBody,
    TreeRequest, WriteTarget, WriteText, WriteTextBody,
};
use serde_json::{Value, json};

use super::CommandContext;
use super::resolve::{
    invalid_input_error, node_summary, resolve_target, service_error, split_parent_name,
};
use super::support::page_json;
use crate::agent_text::guarded_plain_text_sha;
use crate::state::AppState;

pub async fn list(
    state: &AppState,
    context: &CommandContext,
    target: String,
    depth: Option<i64>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Value, CommandError> {
    let caller = context.caller();
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
            .canonical_children(
                account_id,
                space_id,
                folder.node.id,
                ChildrenRequest { limit, cursor },
            )
            .await
            .map_err(service_error)?;

        let items: Vec<Value> = page.items.iter().map(node_summary).collect();
        let returned = items.len();

        return Ok(json!({
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
        }));
    }

    let page = state
        .files
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

    Ok(json!({
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
    }))
}

pub async fn stat(
    state: &AppState,
    context: &CommandContext,
    target: String,
) -> Result<Value, CommandError> {
    let caller = context.caller();
    let (resolved, path) = resolve_target(state, caller, &target).await?;

    let view = state
        .files
        .resolve_path(caller.account_id(), resolved.space_id(), &path)
        .await
        .map_err(service_error)?;
    let mut node = node_summary(&view);
    if let Some(object) = node.as_object_mut() {
        object.insert(
            "write_lock_sources".to_owned(),
            json!(
                view.write_lock_sources
                    .iter()
                    .map(|source| {
                        json!({
                            "node_id": source.node_id,
                            "name": source.name,
                            "path": source.path,
                        })
                    })
                    .collect::<Vec<_>>()
            ),
        );
    }

    Ok(json!({
        "space": resolved.name(),
        "node": node,
    }))
}

pub async fn mkdir(
    state: &AppState,
    context: &CommandContext,
    target: String,
    parents: bool,
) -> Result<Value, CommandError> {
    let caller = context.caller();
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    if parents {
        let (view, created_paths) = state
            .files
            .create_folder_recursive(account_id, space_id, &path)
            .await
            .map_err(service_error)?;

        return Ok(json!({
            "space": resolved.name(),
            "node": node_summary(&view),
            "created_paths": created_paths,
        }));
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

    Ok(json!({
        "space": resolved.name(),
        "node": node_summary(&view),
    }))
}

pub async fn read(
    state: &AppState,
    context: &CommandContext,
    target: String,
    start_line: Option<i64>,
    max_lines: Option<i64>,
    max_bytes: Option<usize>,
    if_none_match_sha256: Option<String>,
) -> Result<Value, CommandError> {
    let caller = context.caller();
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
            "encrypted text is not readable through text commands".to_owned(),
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
                "encrypted text is not readable through text commands".to_owned(),
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
    Ok(body)
}

/// Resolve `path` to an existing node's write target, or a create target under
/// its parent when it does not exist and `create` is set. Shared by `write`
/// and `append`, which only differ in what they do with the existing view.
async fn resolve_write_target(
    state: &AppState,
    account_id: uuid::Uuid,
    space_id: uuid::Uuid,
    path: &str,
    create: bool,
) -> Result<(WriteTarget, Option<NodeView>), CommandError> {
    let existing = match state.files.resolve_path(account_id, space_id, path).await {
        Ok(view) => Some(view),
        Err(ServiceError::NotFound(_)) => None,
        Err(error) => return Err(service_error(error)),
    };

    let target = match &existing {
        Some(view) => WriteTarget::Existing {
            node_id: view.node.id,
        },
        None => {
            if !create {
                return Err(service_error(ServiceError::NotFound(
                    "text does not exist; pass create=true to create it".to_owned(),
                )));
            }
            let (parent_path, name) = split_parent_name(path)?;
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

    Ok((target, existing))
}

pub async fn write(
    state: &AppState,
    context: &CommandContext,
    target: String,
    content: String,
    create: bool,
    mut expected_sha256: Option<String>,
) -> Result<Value, CommandError> {
    let caller = context.caller();
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let (target, existing) =
        resolve_write_target(state, account_id, space_id, &path, create).await?;

    if let Some(view) = &existing {
        let current_sha = guarded_plain_text_sha(
            state,
            account_id,
            space_id,
            view.node.id,
            expected_sha256.as_deref(),
        )
        .await
        .map_err(service_error)?;
        expected_sha256 = Some(current_sha);
    }

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

    Ok(json!({
        "space": resolved.name(),
        "node": node_summary(&view.node),
        "content_sha256": view.text.content_sha256,
        "byte_len": view.text.byte_len,
        "line_count": view.text.line_count,
    }))
}

pub async fn append(
    state: &AppState,
    context: &CommandContext,
    target: String,
    content: String,
    create: bool,
    ensure_newline: bool,
    expected_sha256: Option<String>,
) -> Result<Value, CommandError> {
    let caller = context.caller();
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let (target, _existing) =
        resolve_write_target(state, account_id, space_id, &path, create).await?;

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

    Ok(json!({
        "space": resolved.name(),
        "node": node_summary(&view.node),
        "appended": true,
        "content_sha256": view.text.content_sha256,
        "byte_len": view.text.byte_len,
        "line_count": view.text.line_count,
    }))
}

pub async fn patch(
    state: &AppState,
    context: &CommandContext,
    target: String,
    edits: Vec<PatchEdit>,
    expected_sha256: Option<String>,
) -> Result<Value, CommandError> {
    let edits = prepare_patch_edits(&edits)?;
    let caller = context.caller();
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

    Ok(json!({
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
    }))
}

pub async fn edit(
    state: &AppState,
    context: &CommandContext,
    target: String,
    edits: Vec<LineEditInput>,
    expected_sha256: Option<String>,
) -> Result<Value, CommandError> {
    let edits = prepare_line_edits(&edits)?;
    let caller = context.caller();
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

    Ok(json!({
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
    }))
}

pub async fn mv(
    state: &AppState,
    context: &CommandContext,
    source: String,
    destination: String,
) -> Result<Value, CommandError> {
    let caller = context.caller();
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

    Ok(json!({
        "space": source_space.name(),
        "node": node_summary(&view),
    }))
}

pub async fn copy(
    state: &AppState,
    context: &CommandContext,
    source: String,
    destination: String,
    recursive: bool,
) -> Result<Value, CommandError> {
    let caller = context.caller();
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

    Ok(json!({
        "space": source_space.name(),
        "source_path": source_path,
        "node": node_summary(&result.node),
        "copied": true,
        "counts": {
            "nodes": result.counts.nodes,
            "texts": result.counts.texts,
            "files": result.counts.files,
        },
    }))
}

pub async fn rm(
    state: &AppState,
    context: &CommandContext,
    target: String,
    recursive: bool,
) -> Result<Value, CommandError> {
    let caller = context.caller();
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

    Ok(json!({
        "space": resolved.name(),
        "path": result.path,
        "deleted": true,
        "purge_after": result.purge_after,
    }))
}

fn parse_patch_mode(raw: Option<&str>) -> Result<PatchMode, CommandError> {
    PatchMode::parse(raw.unwrap_or("unique"))
        .ok_or_else(|| invalid_input_error("mode must be 'unique', 'first', or 'all'"))
}

pub(super) fn prepare_patch_edits(edits: &[PatchEdit]) -> Result<Vec<ServiceEdit>, CommandError> {
    if edits.is_empty() {
        return Err(invalid_input_error("edits must not be empty"));
    }
    edits
        .iter()
        .map(|edit| {
            if edit.old_text.is_empty() {
                return Err(service_error(ServiceError::from(PatchError::EmptyOldText)));
            }
            if edit.old_text == edit.new_text {
                return Err(service_error(ServiceError::from(PatchError::NoOpEdit)));
            }
            Ok(ServiceEdit {
                old_text: edit.old_text.clone(),
                new_text: edit.new_text.clone(),
                mode: parse_patch_mode(edit.mode.as_deref())?,
                expected_count: edit.expected_count,
            })
        })
        .collect()
}

pub(super) fn prepare_line_edits(edits: &[LineEditInput]) -> Result<Vec<LineEdit>, CommandError> {
    if edits.is_empty() {
        return Err(invalid_input_error("edits must not be empty"));
    }
    edits
        .iter()
        .cloned()
        .map(|input| {
            let edit = parse_line_edit(input)?;
            let (start, end) = match &edit {
                LineEdit::InsertBefore { line, .. } | LineEdit::InsertAfter { line, .. } => {
                    (*line, *line)
                }
                LineEdit::ReplaceLines {
                    start_line,
                    end_line,
                    ..
                }
                | LineEdit::DeleteLines {
                    start_line,
                    end_line,
                } => (*start_line, *end_line),
            };
            if start < 1 || end < 1 {
                return Err(invalid_input_error("line numbers must be at least 1"));
            }
            if start > end {
                return Err(invalid_input_error(
                    "start_line must be less than or equal to end_line",
                ));
            }
            Ok(edit)
        })
        .collect()
}

fn parse_line_edit(input: LineEditInput) -> Result<LineEdit, CommandError> {
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

fn required_i64(value: Option<i64>, field: &'static str) -> Result<i64, CommandError> {
    value.ok_or_else(|| invalid_input_error(format!("{field} is required")))
}

fn required_string(value: Option<String>, field: &'static str) -> Result<String, CommandError> {
    value.ok_or_else(|| invalid_input_error(format!("{field} is required")))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn patch_validation_preserves_modes_and_rejects_static_errors() {
        let edits = prepare_patch_edits(&[PatchEdit {
            old_text: "before".to_owned(),
            new_text: "after".to_owned(),
            mode: Some("all".to_owned()),
            expected_count: Some(2),
        }])
        .expect("valid patch");

        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].mode, PatchMode::All);
        assert_eq!(edits[0].expected_count, Some(2));

        let empty = prepare_patch_edits(&[]).expect_err("empty edits are rejected");
        assert_eq!(empty.message, "edits must not be empty");

        let unknown = prepare_patch_edits(&[PatchEdit {
            old_text: "before".to_owned(),
            new_text: "after".to_owned(),
            mode: Some("latest".to_owned()),
            expected_count: None,
        }])
        .expect_err("unknown mode is rejected");
        assert_eq!(unknown.message, "mode must be 'unique', 'first', or 'all'");
    }

    #[test]
    fn line_edit_validation_builds_service_edits_and_rejects_missing_fields() {
        let edits = prepare_line_edits(&[
            LineEditInput {
                op: "insert_before_line".to_owned(),
                line: Some(1),
                start_line: None,
                end_line: None,
                content: Some("first".to_owned()),
            },
            LineEditInput {
                op: "delete_lines".to_owned(),
                line: None,
                start_line: Some(2),
                end_line: Some(3),
                content: None,
            },
        ])
        .expect("valid line edits");

        assert_eq!(edits.len(), 2);
        assert!(matches!(edits[0], LineEdit::InsertBefore { line: 1, .. }));
        assert!(matches!(
            edits[1],
            LineEdit::DeleteLines {
                start_line: 2,
                end_line: 3
            }
        ));

        let missing = prepare_line_edits(&[LineEditInput {
            op: "replace_lines".to_owned(),
            line: None,
            start_line: Some(1),
            end_line: Some(2),
            content: None,
        }])
        .expect_err("content is required");
        assert_eq!(missing.message, "content is required");
    }

    #[test]
    fn line_edit_validation_rejects_invalid_ranges() {
        let zero = prepare_line_edits(&[LineEditInput {
            op: "delete_lines".to_owned(),
            line: None,
            start_line: Some(0),
            end_line: Some(1),
            content: None,
        }])
        .expect_err("line numbers are one based");
        assert_eq!(zero.message, "line numbers must be at least 1");

        let reversed = prepare_line_edits(&[LineEditInput {
            op: "delete_lines".to_owned(),
            line: None,
            start_line: Some(3),
            end_line: Some(2),
            content: None,
        }])
        .expect_err("ranges are ordered");
        assert_eq!(
            reversed.message,
            "start_line must be less than or equal to end_line"
        );
    }
}
