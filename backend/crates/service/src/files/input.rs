//! Command inputs for file-tree operations.
//!
//! Inputs are id-centric (the REST/UI surface). The MCP/CLI surface resolves a
//! `target` string and path segments to ids before calling the service; see
//! [`crate::files::target`].

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
/// basename → `name`). A missing path with `create=false` is a `404` the caller
/// returns without invoking the service. The id-centric (REST) caller always
/// uses [`WriteTarget::Existing`].
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
