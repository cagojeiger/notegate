//! Persistence port for file-tree commands.
//!
//! The `db` crate implements this trait; the service stays free of sqlx/axum.

use std::future::Future;

use chrono::{DateTime, Utc};
use notegate_core::Result as CoreResult;
use notegate_model::{Document, Node, Role};
use uuid::Uuid;

use super::input::{CreateFolder, MoveNode};
use super::output::ChildrenCursor;

/// Persistence and authorization for the file tree. The `db` crate implements
/// this; the service stays free of sqlx/axum.
///
/// Read methods exclude soft-deleted rows. Count methods count only live rows.
/// Mutations are attributed via the trailing account argument.
pub trait FilesStore: Clone + Send + Sync + 'static {
    // --- authorization ---

    /// The caller's live role in a workspace, or `None` if no live grant.
    fn role_for(
        &self,
        workspace_id: Uuid,
        account_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<Role>>> + Send;

    // --- reads ---

    /// The workspace's canonical root node (`parent_id IS NULL`).
    fn root_node(&self, workspace_id: Uuid) -> impl Future<Output = CoreResult<Node>> + Send;

    /// Load a live node by id within a workspace.
    fn find_node(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<Node>>> + Send;

    /// The derived display path of a node (root = `/`), or `None` if not found.
    fn node_path(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<String>>> + Send;

    /// Resolve an absolute path (e.g. `/projects/note.md`) to a live node id in
    /// the workspace, or `None` if it does not resolve to a live node. The root
    /// path (`/` or empty) resolves to the workspace root.
    fn resolve_path(
        &self,
        workspace_id: Uuid,
        path: &str,
    ) -> impl Future<Output = CoreResult<Option<Uuid>>> + Send;

    /// Whether a node has any live direct children.
    fn has_children(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = CoreResult<bool>> + Send;

    /// Count of live direct children of a folder.
    fn count_live_children(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
    ) -> impl Future<Output = CoreResult<usize>> + Send;

    /// A live direct child of `parent_node_id` with the given name, if any
    /// (used for sibling-name conflict detection).
    fn find_live_child_by_name(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> impl Future<Output = CoreResult<Option<Node>>> + Send;

    /// Count of live nodes in a workspace.
    fn count_live_nodes(
        &self,
        workspace_id: Uuid,
    ) -> impl Future<Output = CoreResult<usize>> + Send;

    /// Count of live documents in a workspace.
    fn count_live_documents(
        &self,
        workspace_id: Uuid,
    ) -> impl Future<Output = CoreResult<usize>> + Send;

    /// Sum of `byte_len` over the workspace's live documents.
    fn sum_live_document_bytes(
        &self,
        workspace_id: Uuid,
    ) -> impl Future<Output = CoreResult<usize>> + Send;

    /// Load a live document (node + content) by node id.
    fn find_document(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<(Node, Document)>>> + Send;

    /// A page of live direct children, keyset-ordered by `(sort_order, name, id)`.
    /// Returns up to `limit` rows plus whether more rows follow.
    fn paged_children(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        limit: i64,
        cursor: Option<&ChildrenCursor>,
    ) -> impl Future<Output = CoreResult<(Vec<Node>, bool)>> + Send;

    /// The maximum depth of any live descendant relative to `node_id` (0 if the
    /// node has no live children). Used to validate resulting subtree depth on
    /// move.
    fn subtree_relative_depth(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = CoreResult<usize>> + Send;

    /// Count of live nodes in the subtree rooted at `node_id`, including itself.
    /// Used to enforce the synchronous subtree-delete limit.
    fn subtree_live_count(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = CoreResult<usize>> + Send;

    /// Whether `candidate_id` is `node_id` itself or any descendant of it. Used
    /// to forbid moving a node into itself or its own subtree.
    fn is_self_or_descendant(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        candidate_id: Uuid,
    ) -> impl Future<Output = CoreResult<bool>> + Send;

    // --- mutations (attributed) ---

    /// Insert a folder under `command.parent_node_id`, attributing it.
    fn insert_folder(
        &self,
        workspace_id: Uuid,
        command: &CreateFolder,
        created_by: Uuid,
    ) -> impl Future<Output = CoreResult<Node>> + Send;

    /// Insert a document node plus its `documents` row in one transaction,
    /// attributing both. `content` is empty for `touch` and carries the initial
    /// content for create-on-write.
    fn insert_document(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
        content: &StoredContent,
        created_by: Uuid,
    ) -> impl Future<Output = CoreResult<(Node, Document)>> + Send;

    /// Replace a document's content and pre-computed metrics, attributing the
    /// update on both the document and its node.
    fn save_document_content(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        content: &StoredContent,
        expected_sha256: Option<&str>,
        updated_by: Uuid,
    ) -> impl Future<Output = CoreResult<(Node, Document)>> + Send;

    /// Move/rename a node, updating only its `parent_id`/`name` (O(1); no
    /// descendant path rewrite).
    fn move_node(
        &self,
        workspace_id: Uuid,
        command: &MoveNode,
        updated_by: Uuid,
    ) -> impl Future<Output = CoreResult<Node>> + Send;

    /// Update a node's in-place metadata (`name` and/or `sort_order`) without
    /// changing its parent, attributing the update. `None` fields are left
    /// unchanged. Used by REST `PATCH /nodes/{id}` for rename and custom ordering.
    fn update_node_metadata(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        new_name: Option<&str>,
        new_sort_order: Option<i32>,
        updated_by: Uuid,
    ) -> impl Future<Output = CoreResult<Node>> + Send;

    /// Soft-delete a node (and its live subtree for folders), attributing it and
    /// returning the purge eligibility timestamp.
    fn soft_delete_node(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        deleted_by: Uuid,
    ) -> impl Future<Output = CoreResult<DateTime<Utc>>> + Send;
}

/// Pre-computed document content plus its metrics, handed to the store so the
/// hash/byte/line values the service validated are exactly what is persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredContent {
    pub content_md: String,
    pub content_sha256: String,
    pub byte_len: i32,
    pub line_count: i32,
}
