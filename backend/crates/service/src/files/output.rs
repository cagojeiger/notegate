//! Output views for file-tree commands.
//!
//! Every view carries the derived display `path` (never stored on the node — ADR
//! Option B). Read/patch outputs carry the range/metric fields the spec returns.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use notegate_model::{Document, Node};

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
    pub next_cursor: Option<ChildrenCursor>,
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
