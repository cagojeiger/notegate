//! File command and view data shared by service, db, and api.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Document, Node};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildrenRequest {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateFolder {
    pub parent_node_id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateDocument {
    pub parent_node_id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadDocument {
    pub node_id: Uuid,
    pub start_line: Option<i64>,
    pub max_lines: Option<i64>,
    pub max_bytes: Option<usize>,
    pub if_none_match_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteTarget {
    Existing { node_id: Uuid },
    Create { parent_node_id: Uuid, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteDocument {
    pub target: WriteTarget,
    pub content_md: String,
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchDocument {
    pub node_id: Uuid,
    pub edits: Vec<Edit>,
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveNode {
    pub node_id: Uuid,
    pub new_parent_node_id: Uuid,
    pub new_name: Option<String>,
    pub expected_parent_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteNode {
    pub node_id: Uuid,
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredContent {
    pub content_md: String,
    pub content_sha256: String,
    pub byte_len: i32,
    pub line_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentStats {
    pub content_sha256: String,
    pub byte_len: i32,
    pub line_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub node: Node,
    pub path: String,
    pub has_children: bool,
    pub document: Option<DocumentStats>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentView {
    pub node: NodeView,
    pub document: Document,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildrenCursor {
    pub sort_order: i32,
    pub name: String,
    pub id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildrenPage {
    pub parent: NodeView,
    pub items: Vec<NodeView>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteResult {
    pub node_id: Uuid,
    pub path: String,
    pub purge_after: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadResult {
    pub node: NodeView,
    pub content: Option<ReadContent>,
    pub content_sha256: String,
    pub byte_len: i32,
    pub line_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadContent {
    pub content_md: String,
    pub start_line: i64,
    pub end_line: i64,
    pub returned_lines: i64,
    pub truncated: bool,
    pub next_start_line: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchResult {
    pub node: NodeView,
    pub document: Document,
    pub previous_sha256: String,
    pub edits_applied: usize,
    pub diff: String,
}
