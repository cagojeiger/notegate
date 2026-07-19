//! File command and view data shared by service, db, and api.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{FileEncryptionMode, FileObject, Node, NodeKind, TextObject, TextStorageFormat};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildrenRequest {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListNodesRequest {
    pub kind: Option<NodeKind>,
    pub sort: NodeListSort,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeListSort {
    UpdatedAtDesc,
    NameAsc,
}

impl NodeListSort {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "updated_at_desc" => Some(Self::UpdatedAtDesc),
            "name_asc" => Some(Self::NameAsc),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::UpdatedAtDesc => "updated_at_desc",
            Self::NameAsc => "name_asc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateFolder {
    pub parent_node_id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateText {
    pub parent_node_id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadText {
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
pub enum WriteTextBody {
    Plain(String),
    Encrypted(Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteText {
    pub target: WriteTarget,
    pub body: WriteTextBody,
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendText {
    pub target: WriteTarget,
    pub content: String,
    pub expected_sha256: Option<String>,
    pub ensure_newline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchMode {
    Unique,
    First,
    All,
}

impl PatchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unique => "unique",
            Self::First => "first",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub old_text: String,
    pub new_text: String,
    pub mode: PatchMode,
    pub expected_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchText {
    pub node_id: Uuid,
    pub edits: Vec<Edit>,
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineEdit {
    InsertBefore {
        line: i64,
        content: String,
    },
    InsertAfter {
        line: i64,
        content: String,
    },
    ReplaceLines {
        start_line: i64,
        end_line: i64,
        content: String,
    },
    DeleteLines {
        start_line: i64,
        end_line: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditText {
    pub node_id: Uuid,
    pub edits: Vec<LineEdit>,
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
pub struct CopyNode {
    pub node_id: Uuid,
    pub new_parent_node_id: Uuid,
    pub new_name: String,
    pub recursive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyCounts {
    pub nodes: usize,
    pub texts: usize,
    pub files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteNode {
    pub node_id: Uuid,
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredContent {
    pub body: WriteTextBody,
    pub content_sha256: String,
    pub byte_len: i64,
    pub line_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeginObjectUpload {
    pub parent_node_id: Uuid,
    pub name: String,
    pub byte_len: i64,
    pub media_type: String,
    pub original_filename: Option<String>,
    pub encryption_mode: FileEncryptionMode,
    pub encryption_metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingObjectUpload {
    pub id: Uuid,
    pub object_key: String,
    pub space_id: Uuid,
    pub parent_node_id: Uuid,
    pub requested_by_account_id: Uuid,
    pub name: String,
    pub byte_len: i64,
    pub media_type: String,
    pub original_filename: Option<String>,
    pub encryption_mode: FileEncryptionMode,
    pub encryption_metadata: Option<Value>,
    pub node_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextStats {
    pub content_sha256: String,
    pub byte_len: i64,
    pub line_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStats {
    pub media_type: String,
    pub byte_len: i64,
    pub original_filename: Option<String>,
    pub encryption_mode: FileEncryptionMode,
    pub encryption_metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub node: Node,
    pub path: String,
    pub has_children: bool,
    pub text: Option<TextStats>,
    pub file: Option<FileStats>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextView {
    pub node: NodeView,
    pub text: TextObject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileView {
    pub node: NodeView,
    pub file: FileObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildrenCursor {
    pub sort_order: i32,
    pub name: String,
    pub id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "sort", rename_all = "snake_case")]
pub enum NodeListCursor {
    UpdatedAtDesc {
        kind: Option<NodeKind>,
        updated_at: DateTime<Utc>,
        id: Uuid,
    },
    NameAsc {
        kind: Option<NodeKind>,
        name: String,
        id: Uuid,
    },
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
pub struct NodeListPage {
    pub items: Vec<NodeView>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeReveal {
    pub ancestors: Vec<NodeView>,
    pub target: NodeView,
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
    pub storage_format: TextStorageFormat,
    pub body: ReadTextBody,
    pub content_sha256: String,
    pub byte_len: i64,
    pub line_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadTextBody {
    Content(ReadContent),
    Encrypted(Value),
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadContent {
    pub content: String,
    pub start_line: i64,
    pub end_line: i64,
    pub returned_lines: i64,
    pub truncated: bool,
    pub next_start_line: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchResult {
    pub node: NodeView,
    pub text: TextObject,
    pub previous_sha256: String,
    pub edits_applied: usize,
    pub diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyResult {
    pub node: NodeView,
    pub counts: CopyCounts,
}
