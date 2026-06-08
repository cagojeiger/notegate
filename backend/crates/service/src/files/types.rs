//! File command input and output DTOs.

use chrono::{DateTime, Utc};
use notegate_model::{Document, Node};
use uuid::Uuid;

/// Children listing request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildrenRequest {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// Create-folder command (`mkdir`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateFolder {
    pub parent_node_id: Uuid,
    pub name: String,
}

/// Create-document command (`touch`): an empty Markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateDocument {
    pub parent_node_id: Uuid,
    pub name: String,
}

/// Read-document command (`read`/`open`) with range and conditional-read fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadDocument {
    pub node_id: Uuid,
    /// 1-based first line to return; defaults to `1`.
    pub start_line: Option<i64>,
    /// Maximum lines to return; clamped to the read limit.
    pub max_lines: Option<i64>,
    /// Maximum bytes to return; clamped to the read limit.
    pub max_bytes: Option<usize>,
    /// If equal to the current content hash, return an `unchanged` response.
    pub if_none_match_sha256: Option<String>,
}

/// Where a `write`/`save` lands: an existing document, or a new one to create.
///
/// The path-centric (MCP) caller resolves the path before calling: a resolved
/// document node becomes [`WriteTarget::Existing`]; a missing path with
/// `create=true` becomes [`WriteTarget::Create`] (dirname → `parent_node_id`,
/// basename → `name`). A missing path with `create=false` is a surface-level
/// error returned without invoking the write service. The id-centric REST
/// document replace endpoint uses [`WriteTarget::Existing`]; the REST node-create
/// endpoint may use [`WriteTarget::Create`] when a new document includes initial
/// content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteTarget {
    /// Replace the content of an existing document node.
    Existing { node_id: Uuid },
    /// Create a new document under `parent_node_id` named `name`, then write.
    Create { parent_node_id: Uuid, name: String },
}

/// Write-document command (`write`/`save`): full content replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteDocument {
    pub target: WriteTarget,
    pub content_md: String,
    /// Optimistic-concurrency guard; conflict if it does not match.
    pub expected_sha256: Option<String>,
}

/// One exact text replacement within a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub old_text: String,
    pub new_text: String,
}

/// Patch-document command (`patch`): exact targeted replacements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchDocument {
    pub node_id: Uuid,
    pub edits: Vec<Edit>,
    /// Optimistic-concurrency guard; checked before matching.
    pub expected_sha256: Option<String>,
}

/// Move/rename command (`mv`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveNode {
    pub node_id: Uuid,
    pub new_parent_node_id: Uuid,
    /// Rename as part of the move; `None` keeps the current name.
    pub new_name: Option<String>,
    /// Optimistic guard; conflict if the node's current parent differs.
    pub expected_parent_id: Option<Uuid>,
}

/// Soft-delete command (`rm`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteNode {
    pub node_id: Uuid,
    /// Folder deletion requires `recursive=true`.
    pub recursive: bool,
}

/// Lightweight document metrics exposed by single-node `stat` outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentStats {
    pub content_sha256: String,
    pub byte_len: i32,
    pub line_count: i32,
}

/// A node plus its derived display path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub node: Node,
    /// Derived from the parent chain at read time.
    pub path: String,
    pub has_children: bool,
    /// Filled for document `stat`/path-resolution outputs; omitted from bulk `ls`.
    pub document: Option<DocumentStats>,
}

/// A node-with-document view (used by `stat` of a document and after mutations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentView {
    pub node: NodeView,
    pub document: Document,
}

/// Keyset cursor over `(sort_order, name, id)` for children listing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChildrenCursor {
    pub sort_order: i32,
    pub name: String,
    pub id: Uuid,
}

/// A page of child nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildrenPage {
    pub parent: NodeView,
    pub items: Vec<NodeView>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// Result of `rm`: the root node is hidden immediately and eligible for hard
/// purge at `purge_after`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteResult {
    pub node_id: Uuid,
    pub path: String,
    pub purge_after: DateTime<Utc>,
}

/// The result of a `read`/`open`: either a bounded content slice, or an
/// `unchanged` response when `if_none_match_sha256` matched the current hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadResult {
    pub node: NodeView,
    /// `None` when `unchanged` (the caller's hash matched); `Some` otherwise.
    pub content: Option<ReadContent>,
    pub content_sha256: String,
    pub byte_len: i32,
    pub line_count: i32,
}

impl ReadResult {
    /// Whether content was withheld because it was unchanged.
    pub fn unchanged(&self) -> bool {
        self.content.is_none()
    }
}

/// The bounded content slice returned by `read`/`open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadContent {
    pub content_md: String,
    pub start_line: i64,
    pub end_line: i64,
    pub returned_lines: i64,
    pub truncated: bool,
    pub next_start_line: Option<i64>,
}

/// The result of a successful `patch`: the new metrics plus the previous hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchResult {
    pub node: NodeView,
    pub document: Document,
    pub previous_sha256: String,
    pub edits_applied: usize,
    pub diff: String,
}
