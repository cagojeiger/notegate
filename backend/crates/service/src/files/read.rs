use notegate_core::limits;
use notegate_model::NodeKind;
use uuid::Uuid;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::files::validation;
use crate::files::{
    ChildrenCursor, ChildrenPage, ChildrenRequest, FileCommand, NodeView, ReadContent, ReadResult,
    ReadText, ReadTextBody,
};

use super::{FilesService, join_path};

impl FilesService {
    /// Metadata for a node (`stat`). Requires read permission.
    pub async fn stat(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<NodeView> {
        self.authorize(space_id, caller_account_id, FileCommand::Stat)
            .await?;
        let node = self.load_node(space_id, node_id).await?;
        self.node_view(space_id, node).await
    }

    /// Resolve an absolute path to a live node and return its view. Requires
    /// read permission. A path that does not resolve to a live node is not-found
    /// (`404`). Deleted nodes are not resolved.
    pub async fn resolve_path(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        path: &str,
    ) -> ServiceResult<NodeView> {
        self.authorize(space_id, caller_account_id, FileCommand::Stat)
            .await?;
        let path = validation::normalize_path(path)?;
        let node_id = self
            .store
            .resolve_path(space_id, &path)
            .await?
            .ok_or_else(|| ServiceError::NotFound("path does not resolve to a node".to_owned()))?;
        let node = self.load_node(space_id, node_id).await?;
        self.node_view(space_id, node).await
    }

    /// List a folder's direct children (`ls`), keyset-paginated. Requires read permission.
    pub async fn children(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        parent_node_id: Uuid,
        request: ChildrenRequest,
    ) -> ServiceResult<ChildrenPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Ls)
            .await?;

        let parent = self.load_node(space_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(ServiceError::InvalidInput(
                "cannot list children of a text".to_owned(),
            ));
        }
        let parent_path = self.path_of(space_id, parent_node_id).await?;
        let parent_has_children = self.store.has_children(space_id, parent_node_id).await?;

        let limit = clamp_children_limit(request.limit);
        let cursor = match request.cursor.as_deref() {
            None => None,
            Some(raw) => Some(cursor::decode::<ChildrenCursor>(raw)?),
        };
        let (rows, has_more) = self
            .store
            .paged_children(space_id, parent_node_id, limit, cursor.as_ref())
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
            let has_children = self.store.has_children(space_id, node.id).await?;
            items.push(NodeView {
                node,
                path,
                has_children,
                text: None,
            });
        }

        Ok(ChildrenPage {
            parent: NodeView {
                node: parent,
                path: parent_path,
                has_children: parent_has_children,
                text: None,
            },
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    /// Read a text with range limits (`read`/`open`). Requires read permission.
    pub async fn read_text(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: ReadText,
    ) -> ServiceResult<ReadResult> {
        self.authorize(space_id, caller_account_id, FileCommand::Read)
            .await?;
        let (node, text) = self.load_text(space_id, command.node_id).await?;
        let view = self.text_node_view(space_id, node, &text).await?;

        // Conditional read: unchanged when the caller's hash matches.
        if let Some(ref hash) = command.if_none_match_sha256
            && hash == &text.content_sha256
        {
            return Ok(ReadResult {
                node: view,
                storage_format: text.storage_format,
                body: ReadTextBody::Unchanged,
                content_sha256: text.content_sha256,
                byte_len: text.byte_len,
                line_count: text.line_count,
            });
        }

        let body = if let Some(plain_content) = text.content.as_deref() {
            ReadTextBody::Content(slice_text(
                plain_content,
                command.start_line,
                command.max_lines,
                command.max_bytes,
            )?)
        } else if let Some(payload) = text.encrypted_payload {
            ReadTextBody::Encrypted(payload)
        } else {
            return Err(ServiceError::InvalidInput(
                "text has neither plaintext nor encrypted payload".to_owned(),
            ));
        };

        Ok(ReadResult {
            node: view,
            storage_format: text.storage_format,
            body,
            content_sha256: text.content_sha256,
            byte_len: text.byte_len,
            line_count: text.line_count,
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

/// Slice a text by a 1-based line range and a byte budget, reporting whether
/// the result was truncated and the next start line.
pub(super) fn slice_text(
    content: &str,
    start_line: Option<i64>,
    max_lines: Option<i64>,
    max_bytes: Option<usize>,
) -> ServiceResult<ReadContent> {
    let start_line = start_line.unwrap_or(1).max(1);
    let max_lines = match max_lines {
        None => limits::READ_DEFAULT_MAX_LINES,
        Some(value) if value < 1 => 1,
        Some(value) => value.min(limits::READ_MAX_LINES),
    };
    let max_bytes = match max_bytes {
        None => limits::READ_DEFAULT_MAX_BYTES,
        Some(0) => {
            return Err(ServiceError::InvalidInput(
                "max_bytes must be at least 1".to_owned(),
            ));
        }
        Some(value) => value.min(limits::READ_MAX_BYTES),
    };

    // Split into logical line byte ranges, preserving the stored line endings.
    let lines = line_ranges(content);
    let total_lines = lines.len() as i64;

    if total_lines == 0 || start_line > total_lines {
        return Ok(ReadContent {
            content: String::new(),
            start_line,
            end_line: start_line.saturating_sub(1),
            returned_lines: 0,
            truncated: false,
            next_start_line: None,
        });
    }

    let start_index = (start_line - 1) as usize;
    let mut out = String::new();
    let mut returned = 0_i64;

    for range in lines.iter().skip(start_index).take(max_lines as usize) {
        let Some(line) = content.get(range.clone()) else {
            return Err(ServiceError::Internal(
                "failed to slice text at line boundary".to_owned(),
            ));
        };
        let candidate_len = line.len();
        if !out.is_empty() && out.len() + candidate_len > max_bytes {
            // Byte budget reached after at least one line; stop here.
            break;
        }
        out.push_str(line);
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

    Ok(ReadContent {
        content: out,
        start_line,
        end_line,
        returned_lines: returned,
        truncated,
        next_start_line,
    })
}

/// Split content into logical line byte ranges, preserving the original line
/// endings. A trailing `\n` belongs to the last logical line instead of creating
/// an extra empty line, mirroring [`content::compute`](crate::files::content::compute)'s
/// line count.
fn line_ranges(content: &str) -> Vec<std::ops::Range<usize>> {
    if content.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut start = 0;
    for (idx, ch) in content.char_indices() {
        if ch == '\n' {
            let end = idx + ch.len_utf8();
            ranges.push(start..end);
            start = end;
        }
    }
    if start < content.len() {
        ranges.push(start..content.len());
    }
    ranges
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]

    use super::*;

    #[test]
    fn slice_preserves_stored_line_endings() {
        let content =
            slice_text("a\r\nb\nc", Some(1), Some(3), None).expect("slice should succeed");

        assert_eq!(content.content, "a\r\nb\nc");
        assert_eq!(content.returned_lines, 3);
        assert!(!content.truncated);
    }

    #[test]
    fn slice_rejects_zero_max_bytes() {
        let err = slice_text("a\n", None, None, Some(0)).unwrap_err();

        assert!(
            matches!(err, ServiceError::InvalidInput(message) if message == "max_bytes must be at least 1")
        );
    }

    #[test]
    fn slice_returns_at_least_one_full_line_for_forward_progress() {
        let content = slice_text("long-line\nnext\n", Some(1), Some(10), Some(4)).expect("slice");

        assert_eq!(content.content, "long-line\n");
        assert_eq!(content.returned_lines, 1);
        assert!(content.truncated);
        assert_eq!(content.next_start_line, Some(2));
    }
}
