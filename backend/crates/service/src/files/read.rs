use notegate_core::limits;
use notegate_model::NodeKind;
use uuid::Uuid;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::files::validation;
use crate::files::{
    ChildrenCursor, ChildrenPage, ChildrenRequest, FileCommand, NodeView, ReadContent,
    ReadDocument, ReadResult,
};

use super::{FilesService, join_path};

impl FilesService {
    /// Metadata for a node (`stat`). Requires `viewer`.
    pub async fn stat(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<NodeView> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Stat)
            .await?;
        let node = self.load_node(workspace_id, node_id).await?;
        self.node_view(workspace_id, node).await
    }

    /// Resolve an absolute path to a live node and return its view. Requires
    /// `viewer`. A path that does not resolve to a live node is not-found
    /// (`404`). Deleted nodes are not resolved.
    pub async fn resolve_path(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        path: &str,
    ) -> ServiceResult<NodeView> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Stat)
            .await?;
        let path = validation::normalize_path(path)?;
        let node_id = self
            .store
            .resolve_path(workspace_id, &path)
            .await?
            .ok_or_else(|| ServiceError::NotFound("path does not resolve to a node".to_owned()))?;
        let node = self.load_node(workspace_id, node_id).await?;
        self.node_view(workspace_id, node).await
    }

    /// List a folder's direct children (`ls`), keyset-paginated. Requires `viewer`.
    pub async fn children(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        request: ChildrenRequest,
    ) -> ServiceResult<ChildrenPage> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Ls)
            .await?;

        let parent = self.load_node(workspace_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(ServiceError::InvalidInput(
                "cannot list children of a document".to_owned(),
            ));
        }
        let parent_path = self.path_of(workspace_id, parent_node_id).await?;
        let parent_has_children = self
            .store
            .has_children(workspace_id, parent_node_id)
            .await?;

        let limit = clamp_children_limit(request.limit);
        let cursor = match request.cursor.as_deref() {
            None => None,
            Some(raw) => Some(cursor::decode::<ChildrenCursor>(raw)?),
        };
        let (rows, has_more) = self
            .store
            .paged_children(workspace_id, parent_node_id, limit, cursor.as_ref())
            .await?;

        let next_cursor = if has_more {
            rows.last()
                .map(|node| ChildrenCursor {
                    sort_order: node.sort_order,
                    name: node.name.clone(),
                    id: node.id,
                })
                .map(|cursor| cursor::encode(&cursor))
                .transpose()
                .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
        } else {
            None
        };

        let mut items = Vec::with_capacity(rows.len());
        for node in rows {
            let path = join_path(&parent_path, &node.name);
            let has_children = self.store.has_children(workspace_id, node.id).await?;
            items.push(NodeView {
                node,
                path,
                has_children,
                document: None,
            });
        }

        Ok(ChildrenPage {
            parent: NodeView {
                node: parent,
                path: parent_path,
                has_children: parent_has_children,
                document: None,
            },
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    /// Read a document with range limits (`read`/`open`). Requires `viewer`.
    pub async fn read_document(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        command: ReadDocument,
    ) -> ServiceResult<ReadResult> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Read)
            .await?;
        let (node, document) = self.load_document(workspace_id, command.node_id).await?;
        let view = self
            .document_node_view(workspace_id, node, &document)
            .await?;

        // Conditional read: unchanged when the caller's hash matches.
        if let Some(ref hash) = command.if_none_match_sha256
            && hash == &document.content_sha256
        {
            return Ok(ReadResult {
                node: view,
                content: None,
                content_sha256: document.content_sha256,
                byte_len: document.byte_len,
                line_count: document.line_count,
            });
        }

        let content = slice_document(
            &document.content_md,
            command.start_line,
            command.max_lines,
            command.max_bytes,
        );

        Ok(ReadResult {
            node: view,
            content: Some(content),
            content_sha256: document.content_sha256,
            byte_len: document.byte_len,
            line_count: document.line_count,
        })
    }
}

/// Clamp a children-listing limit to `1..=CHILDREN_MAX_LIMIT`, defaulting to
/// [`limits::CHILDREN_DEFAULT_LIMIT`].
fn clamp_children_limit(limit: Option<i64>) -> i64 {
    match limit {
        None => limits::CHILDREN_DEFAULT_LIMIT,
        Some(value) if value < 1 => 1,
        Some(value) => value.min(limits::CHILDREN_MAX_LIMIT),
    }
}

/// Slice a document by a 1-based line range and a byte budget, reporting whether
/// the result was truncated and the next start line.
pub(super) fn slice_document(
    content: &str,
    start_line: Option<i64>,
    max_lines: Option<i64>,
    max_bytes: Option<usize>,
) -> ReadContent {
    let start_line = start_line.unwrap_or(1).max(1);
    let max_lines = match max_lines {
        None => limits::READ_DEFAULT_MAX_LINES,
        Some(value) if value < 1 => 1,
        Some(value) => value.min(limits::READ_MAX_LINES),
    };
    let max_bytes = match max_bytes {
        None => limits::READ_DEFAULT_MAX_BYTES,
        Some(value) => value.min(limits::READ_MAX_BYTES),
    };

    // Split into lines preserving the logical line count used elsewhere.
    let lines = split_lines(content);
    let total_lines = lines.len() as i64;

    if total_lines == 0 || start_line > total_lines {
        return ReadContent {
            content_md: String::new(),
            start_line,
            end_line: start_line.saturating_sub(1),
            returned_lines: 0,
            truncated: false,
            next_start_line: None,
        };
    }

    let start_index = (start_line - 1) as usize;
    let mut out = String::new();
    let mut returned = 0_i64;

    for line in lines.iter().skip(start_index).take(max_lines as usize) {
        // Re-add the newline that `split_lines` stripped, reconstructing exactly
        // one '\n' between lines as the canonical separator.
        let candidate_len = line.len() + 1;
        if !out.is_empty() && out.len() + candidate_len > max_bytes {
            // Byte budget reached after at least one line; stop here.
            break;
        }
        out.push_str(line);
        out.push('\n');
        returned += 1;
        if out.len() >= max_bytes {
            // Always return at least one line (forward progress), then stop once
            // the byte budget is met or exceeded.
            break;
        }
    }

    let end_line = start_line + returned - 1;
    // Truncated whenever any line beyond what we returned remains (whether the
    // stop was the line cap or the byte budget).
    let truncated = (start_index as i64 + returned) < total_lines;
    let next_start_line = if truncated { Some(end_line + 1) } else { None };

    ReadContent {
        content_md: out,
        start_line,
        end_line,
        returned_lines: returned,
        truncated,
        next_start_line,
    }
}

/// Split content into logical lines (drops the single trailing newline so a
/// document ending in `\n` is not counted as having a trailing empty line). This
/// mirrors [`content::compute`](crate::files::content::compute)'s line count.
fn split_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    let trimmed = content.strip_suffix('\n').unwrap_or(content);
    trimmed.split('\n').collect()
}
