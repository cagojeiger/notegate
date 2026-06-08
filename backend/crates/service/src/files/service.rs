//! File-tree command service: `ls` / `stat` / `mkdir` / `touch` / `read` /
//! `write` / `patch` / `mv` / `rm` / `restore`.
//!
//! Every command takes `(caller_account_id, workspace_id, ...)`. The service:
//!
//! 1. Resolves the caller's live [`Role`] via [`FilesStore::role_for`] FIRST. No
//!    live role ⇒ not-found (`404`, hides the workspace); an insufficient role ⇒
//!    forbidden (`403`, via [`policy::require`]).
//! 2. Validates input format (name, `.md`, depth, path length, document size)
//!    with the pure [`validation`] functions.
//! 3. Pre-checks capacity limits (fanout, node/document counts, total bytes,
//!    subtree-delete size) using counts read from the store, returning a typed
//!    conflict. The DB layer re-enforces these in-transaction for race safety;
//!    the service pre-check keeps the logic testable and the errors precise.
//! 4. Calls the store mutation, attributing it to the caller.
//!
//! Paths are never stored on a node — the display path is derived from parents;
//! `move`/`rename` change only the moved node's `parent_id`/`name`.

use notegate_model::{Document, Node, NodeKind, Role};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};

use super::content;
use super::input::{
    ChildrenRequest, CreateDocument, CreateFolder, DeleteNode, MoveNode, PatchDocument,
    ReadDocument, RestoreNode, WriteDocument, WriteTarget,
};
use super::output::{
    ChildrenCursor, ChildrenPage, DocumentView, NodeView, PatchResult, ReadContent, ReadResult,
};
use super::patch::{apply_edits, unified_diff};
use super::policy::{self, FileCommand};
use super::store::FilesStore;
use super::validation;

use notegate_core::limits;

/// File-tree service.
#[derive(Debug, Clone)]
pub struct FilesService<S> {
    store: S,
}

impl<S> FilesService<S>
where
    S: FilesStore,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// The workspace root node, as a view. Requires `viewer`.
    pub async fn root(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
    ) -> ServiceResult<NodeView> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Stat)
            .await?;
        let node = self.store.root_node(workspace_id).await?;
        let has_children = self.store.has_children(workspace_id, node.id).await?;
        Ok(NodeView {
            node,
            path: "/".to_owned(),
            has_children,
        })
    }

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
        let node_id = self
            .store
            .resolve_path(workspace_id, path)
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
        let (rows, has_more) = self
            .store
            .paged_children(workspace_id, parent_node_id, limit, request.cursor.as_ref())
            .await?;

        let next_cursor = if has_more {
            rows.last().map(|node| ChildrenCursor {
                sort_order: node.sort_order,
                name: node.name.clone(),
                id: node.id,
            })
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
            });
        }

        Ok(ChildrenPage {
            parent: NodeView {
                node: parent,
                path: parent_path,
                has_children: parent_has_children,
            },
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    /// Create a folder (`mkdir`). Requires `editor`.
    pub async fn create_folder(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        command: CreateFolder,
    ) -> ServiceResult<NodeView> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Mkdir)
            .await?;
        validation::validate_basename(&command.name, NodeKind::Folder)?;

        let parent_path = self
            .prepare_create(workspace_id, command.parent_node_id, &command.name)
            .await?;

        let node = self
            .store
            .insert_folder(workspace_id, &command, caller_account_id)
            .await?;
        let path = join_path(&parent_path, &node.name);
        Ok(NodeView {
            node,
            path,
            has_children: false,
        })
    }

    /// Create an empty document (`touch`). Requires `editor`.
    pub async fn create_document(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        command: CreateDocument,
    ) -> ServiceResult<DocumentView> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Touch)
            .await?;
        validation::validate_basename(&command.name, NodeKind::Document)?;

        let parent_path = self
            .prepare_create(workspace_id, command.parent_node_id, &command.name)
            .await?;

        // A document also consumes the live-document quota.
        let documents = self.store.count_live_documents(workspace_id).await?;
        validation::validate_workspace_document_count(documents)?;

        let empty = content::compute("").into_stored(String::new());
        let (node, document) = self
            .store
            .insert_document(
                workspace_id,
                command.parent_node_id,
                &command.name,
                &empty,
                caller_account_id,
            )
            .await?;
        let path = join_path(&parent_path, &node.name);
        Ok(DocumentView {
            node: NodeView {
                node,
                path,
                has_children: false,
            },
            document,
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
        let view = self.node_view(workspace_id, node).await?;

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

    /// Replace a document's content (`write`/`save`). Requires `editor`.
    ///
    /// [`WriteTarget::Existing`] replaces an existing document (the `create=false`
    /// case, and the resolved `create=true` case). [`WriteTarget::Create`] makes a
    /// new document, re-checking node/document/fanout/depth/name limits. Both
    /// enforce the per-document and workspace-total content caps; the existing
    /// case also honors the `expected_sha256` optimistic guard.
    pub async fn write_document(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        command: WriteDocument,
    ) -> ServiceResult<DocumentView> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Write)
            .await?;

        let metrics = content::compute(&command.content_md);
        validation::validate_document_content(metrics.byte_len, metrics.line_count)?;

        match command.target {
            WriteTarget::Existing { node_id } => {
                let (node, document) = self.load_document(workspace_id, node_id).await?;
                check_expected_sha(command.expected_sha256.as_deref(), &document.content_sha256)?;

                let total = self.store.sum_live_document_bytes(workspace_id).await?;
                validation::validate_workspace_document_bytes(
                    total,
                    document.byte_len.max(0) as usize,
                    metrics.byte_len,
                )?;

                let stored = metrics.into_stored(command.content_md);
                let (node, document) = self
                    .store
                    .save_document_content(workspace_id, node.id, &stored, caller_account_id)
                    .await?;
                self.document_view(workspace_id, node, document).await
            }
            WriteTarget::Create {
                parent_node_id,
                name,
            } => {
                // expected_sha256 cannot match a not-yet-existent document.
                if command.expected_sha256.is_some() {
                    return Err(ServiceError::Conflict(
                        "expected_sha256 was supplied but the document does not exist".to_owned(),
                    ));
                }
                validation::validate_basename(&name, NodeKind::Document)?;
                self.prepare_create(workspace_id, parent_node_id, &name)
                    .await?;

                // New-document quotas: live-document count and total byte budget.
                let documents = self.store.count_live_documents(workspace_id).await?;
                validation::validate_workspace_document_count(documents)?;
                let total = self.store.sum_live_document_bytes(workspace_id).await?;
                validation::validate_workspace_document_bytes(total, 0, metrics.byte_len)?;

                let stored = metrics.into_stored(command.content_md);
                let (node, document) = self
                    .store
                    .insert_document(
                        workspace_id,
                        parent_node_id,
                        &name,
                        &stored,
                        caller_account_id,
                    )
                    .await?;
                self.document_view(workspace_id, node, document).await
            }
        }
    }

    /// Apply exact targeted edits to a document (`patch`). Requires `editor`.
    pub async fn patch_document(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        command: PatchDocument,
    ) -> ServiceResult<PatchResult> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Patch)
            .await?;

        if command.edits.is_empty() {
            return Err(ServiceError::InvalidInput(
                "edits must not be empty".to_owned(),
            ));
        }

        let (node, document) = self.load_document(workspace_id, command.node_id).await?;
        let previous_sha256 = document.content_sha256.clone();

        // expected_sha256 is checked before any matching.
        check_expected_sha(command.expected_sha256.as_deref(), &previous_sha256)?;

        let new_content = apply_edits(&document.content_md, &command.edits)?;
        let diff = unified_diff(&document.content_md, &new_content);

        let metrics = content::compute(&new_content);
        validation::validate_document_content(metrics.byte_len, metrics.line_count)?;

        let total = self.store.sum_live_document_bytes(workspace_id).await?;
        validation::validate_workspace_document_bytes(
            total,
            document.byte_len.max(0) as usize,
            metrics.byte_len,
        )?;

        let stored = metrics.into_stored(new_content);
        let (node, document) = self
            .store
            .save_document_content(workspace_id, node.id, &stored, caller_account_id)
            .await?;
        let view = self.document_view(workspace_id, node, document).await?;

        Ok(PatchResult {
            node: view.node,
            document: view.document,
            previous_sha256,
            edits_applied: command.edits.len(),
            diff,
        })
    }

    /// Move or rename a node (`mv`). Requires `editor`.
    pub async fn move_node(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        command: MoveNode,
    ) -> ServiceResult<NodeView> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Mv)
            .await?;

        let node = self.load_node(workspace_id, command.node_id).await?;
        if node.parent_id.is_none() {
            return Err(ServiceError::Conflict(
                "cannot move the root node".to_owned(),
            ));
        }

        let dest_parent = self
            .load_node(workspace_id, command.new_parent_node_id)
            .await?;
        if dest_parent.kind != NodeKind::Folder {
            return Err(ServiceError::InvalidInput(
                "destination parent must be a folder".to_owned(),
            ));
        }

        let final_name = command
            .new_name
            .clone()
            .unwrap_or_else(|| node.name.clone());
        validation::validate_basename(&final_name, node.kind)?;

        // Same parent and same name ⇒ no-op success.
        if node.parent_id == Some(command.new_parent_node_id) && final_name == node.name {
            return self.node_view(workspace_id, node).await;
        }

        // Cannot move a node into itself or its own descendant.
        if self
            .store
            .is_self_or_descendant(workspace_id, command.node_id, command.new_parent_node_id)
            .await?
        {
            return Err(ServiceError::Conflict(
                "cannot move a node into itself or its descendant".to_owned(),
            ));
        }

        // Destination sibling-name conflict (ignore the node itself for in-place
        // rename within the same parent).
        if let Some(conflict) = self
            .store
            .find_live_child_by_name(workspace_id, command.new_parent_node_id, &final_name)
            .await?
            && conflict.id != node.id
        {
            return Err(ServiceError::Conflict(format!(
                "destination already has a node named '{final_name}'"
            )));
        }

        // Resulting subtree depth and path length.
        let dest_parent_path = self
            .path_of(workspace_id, command.new_parent_node_id)
            .await?;
        let dest_parent_depth = path_depth(&dest_parent_path);
        let new_path = join_path(&dest_parent_path, &final_name);
        validation::validate_path_len(&new_path)?;
        let subtree_depth = self
            .store
            .subtree_relative_depth(workspace_id, command.node_id)
            .await?;
        validation::validate_depth(dest_parent_depth + 1 + subtree_depth)?;

        // Destination fanout (only when actually changing parent).
        if node.parent_id != Some(command.new_parent_node_id) {
            let children = self
                .store
                .count_live_children(workspace_id, command.new_parent_node_id)
                .await?;
            validation::validate_fanout(children)?;
        }

        let moved = self
            .store
            .move_node(workspace_id, &command, caller_account_id)
            .await?;
        self.node_view(workspace_id, moved).await
    }

    /// Update a node's in-place metadata: rename and/or reorder (`PATCH`).
    /// Requires `editor`. The node keeps its parent. Renaming the root is
    /// rejected; a rename validates the new basename and sibling-name uniqueness.
    /// At least one of `new_name`/`new_sort_order` must be present.
    pub async fn update_node(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        node_id: Uuid,
        new_name: Option<String>,
        new_sort_order: Option<i32>,
    ) -> ServiceResult<NodeView> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Mv)
            .await?;

        if new_name.is_none() && new_sort_order.is_none() {
            return Err(ServiceError::InvalidInput(
                "provide name and/or sort_order to update".to_owned(),
            ));
        }

        let node = self.load_node(workspace_id, node_id).await?;

        if let Some(ref name) = new_name {
            if node.parent_id.is_none() {
                return Err(ServiceError::Conflict(
                    "cannot rename the root node".to_owned(),
                ));
            }
            validation::validate_basename(name, node.kind)?;

            // Renaming to the same name is allowed (no-op for the name); only a
            // *different* live sibling with that name is a conflict.
            if *name != node.name
                && let Some(parent_id) = node.parent_id
                && let Some(conflict) = self
                    .store
                    .find_live_child_by_name(workspace_id, parent_id, name)
                    .await?
                && conflict.id != node.id
            {
                return Err(ServiceError::Conflict(format!(
                    "a node named '{name}' already exists in this folder"
                )));
            }
        }

        let updated = self
            .store
            .update_node_metadata(
                workspace_id,
                node_id,
                new_name.as_deref(),
                new_sort_order,
                caller_account_id,
            )
            .await?;
        self.node_view(workspace_id, updated).await
    }

    /// Soft-delete a node (`rm`). Requires `editor`.
    pub async fn delete_node(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        command: DeleteNode,
    ) -> ServiceResult<()> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Rm)
            .await?;

        let node = self.load_node(workspace_id, command.node_id).await?;
        if node.parent_id.is_none() {
            return Err(ServiceError::Conflict(
                "cannot delete the root node".to_owned(),
            ));
        }

        if node.kind == NodeKind::Folder {
            if !command.recursive {
                return Err(ServiceError::Conflict(
                    "folder deletion requires recursive=true".to_owned(),
                ));
            }
            let subtree = self.store.subtree_live_count(workspace_id, node.id).await?;
            if subtree > limits::SUBTREE_DELETE_MAX_NODES {
                return Err(ServiceError::Conflict(format!(
                    "subtree of {subtree} nodes exceeds the synchronous delete limit of {}; narrow the operation",
                    limits::SUBTREE_DELETE_MAX_NODES
                )));
            }
        }

        self.store
            .soft_delete_node(workspace_id, node.id, caller_account_id)
            .await?;
        Ok(())
    }

    /// Restore a soft-deleted node (`restore`). Requires `editor`.
    ///
    /// Re-validates sibling-name uniqueness, fanout, and resulting depth against
    /// the current (live) tree, and rejects the restore if any ancestor is still
    /// soft-deleted (a deleted parent chain would orphan the node) with an
    /// actionable hint to restore the ancestor first.
    pub async fn restore_node(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        command: RestoreNode,
    ) -> ServiceResult<NodeView> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Restore)
            .await?;

        let node = self
            .store
            .find_deleted_node(workspace_id, command.node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("deleted node not found".to_owned()))?;

        let Some(parent_id) = node.parent_id else {
            return Err(ServiceError::Conflict(
                "the root node is never deleted".to_owned(),
            ));
        };

        // Reject when the parent chain is still deleted.
        if self
            .store
            .has_deleted_ancestor(workspace_id, command.node_id)
            .await?
        {
            return Err(ServiceError::Conflict(
                "an ancestor is still deleted; restore the ancestor folder first".to_owned(),
            ));
        }

        // Sibling-name uniqueness against the now-live parent.
        if let Some(conflict) = self
            .store
            .find_live_child_by_name(workspace_id, parent_id, &node.name)
            .await?
            && conflict.id != node.id
        {
            return Err(ServiceError::Conflict(format!(
                "a live node named '{}' already exists; rename before restoring",
                node.name
            )));
        }

        // Fanout and resulting depth.
        let children = self
            .store
            .count_live_children(workspace_id, parent_id)
            .await?;
        validation::validate_fanout(children)?;

        let parent_path = self.path_of(workspace_id, parent_id).await?;
        let parent_depth = path_depth(&parent_path);
        let subtree_depth = self
            .store
            .subtree_relative_depth(workspace_id, command.node_id)
            .await?;
        validation::validate_depth(parent_depth + 1 + subtree_depth)?;

        let restored = self
            .store
            .restore_node(workspace_id, command.node_id, caller_account_id)
            .await?;
        self.node_view(workspace_id, restored).await
    }

    // --- internal helpers ---

    /// Resolve the caller's role (no role ⇒ 404) and gate by command (lesser
    /// role ⇒ 403).
    async fn authorize(
        &self,
        workspace_id: Uuid,
        account_id: Uuid,
        command: FileCommand,
    ) -> ServiceResult<Role> {
        let role = self
            .store
            .role_for(workspace_id, account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("workspace not found".to_owned()))?;
        policy::require(role, command)?;
        Ok(role)
    }

    /// Load a live node or 404.
    async fn load_node(&self, workspace_id: Uuid, node_id: Uuid) -> ServiceResult<Node> {
        self.store
            .find_node(workspace_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))
    }

    /// Load a live document or 404 (also 404 when the node is a folder).
    async fn load_document(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<(Node, Document)> {
        self.store
            .find_document(workspace_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("document not found".to_owned()))
    }

    /// The derived path of a node or 404.
    async fn path_of(&self, workspace_id: Uuid, node_id: Uuid) -> ServiceResult<String> {
        self.store
            .node_path(workspace_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))
    }

    /// Build a [`NodeView`] for an existing node (derives path + has_children).
    async fn node_view(&self, workspace_id: Uuid, node: Node) -> ServiceResult<NodeView> {
        let path = self.path_of(workspace_id, node.id).await?;
        let has_children = self.store.has_children(workspace_id, node.id).await?;
        Ok(NodeView {
            node,
            path,
            has_children,
        })
    }

    /// Build a [`DocumentView`] for an existing document node.
    async fn document_view(
        &self,
        workspace_id: Uuid,
        node: Node,
        document: Document,
    ) -> ServiceResult<DocumentView> {
        let node = self.node_view(workspace_id, node).await?;
        Ok(DocumentView { node, document })
    }

    /// Shared create pre-checks for mkdir/touch/write-create: parent is a live
    /// folder, no sibling-name conflict, resulting depth + path length within
    /// limits, parent fanout and workspace node count within limits. Returns the
    /// parent's derived path.
    async fn prepare_create(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> ServiceResult<String> {
        let parent = self.load_node(workspace_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(ServiceError::InvalidInput(
                "parent must be a folder".to_owned(),
            ));
        }

        // Name conflict against live siblings.
        if self
            .store
            .find_live_child_by_name(workspace_id, parent_node_id, name)
            .await?
            .is_some()
        {
            return Err(ServiceError::Conflict(format!(
                "a node named '{name}' already exists in this folder"
            )));
        }

        let parent_path = self.path_of(workspace_id, parent_node_id).await?;
        let parent_depth = path_depth(&parent_path);
        let new_path = join_path(&parent_path, name);
        validation::validate_depth(parent_depth + 1)?;
        validation::validate_path_len(&new_path)?;

        let children = self
            .store
            .count_live_children(workspace_id, parent_node_id)
            .await?;
        validation::validate_fanout(children)?;

        let nodes = self.store.count_live_nodes(workspace_id).await?;
        validation::validate_workspace_node_count(nodes)?;

        Ok(parent_path)
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

/// Join a parent path and a child name into a canonical path (root-aware).
fn join_path(parent_path: &str, name: &str) -> String {
    if parent_path == "/" {
        format!("/{name}")
    } else {
        format!("{parent_path}/{name}")
    }
}

/// Depth (segment count below root) of a derived path. Root (`/`) is 0.
fn path_depth(path: &str) -> usize {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .count()
}

/// Compare an optional `expected_sha256` to the current hash; conflict on mismatch.
fn check_expected_sha(expected: Option<&str>, current: &str) -> ServiceResult<()> {
    if let Some(expected) = expected
        && expected != current
    {
        return Err(ServiceError::Conflict(
            "expected_sha256 does not match the current document; read it again".to_owned(),
        ));
    }
    Ok(())
}

/// Slice a document by a 1-based line range and a byte budget, reporting whether
/// the result was truncated and the next start line.
fn slice_document(
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
/// mirrors [`content_metrics`]'s line count.
fn split_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    let trimmed = content.strip_suffix('\n').unwrap_or(content);
    trimmed.split('\n').collect()
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
    use crate::files::StoredContent;
    use chrono::Utc;
    use notegate_core::Result as CoreResult;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use crate::files::input::Edit;

    /// An in-memory file tree for exercising the service end-to-end without a DB.
    #[derive(Clone)]
    struct MemStore {
        role: Option<Role>,
        workspace_id: Uuid,
        root_id: Uuid,
        state: Arc<Mutex<State>>,
    }

    #[derive(Default)]
    struct State {
        nodes: HashMap<Uuid, Node>,
        documents: HashMap<Uuid, Document>,
    }

    fn actor() -> Uuid {
        Uuid::nil()
    }

    impl MemStore {
        fn new(role: Option<Role>) -> Self {
            let workspace_id = Uuid::new_v4();
            let root_id = Uuid::new_v4();
            let mut nodes = HashMap::new();
            nodes.insert(
                root_id,
                raw_node(root_id, workspace_id, None, "/", NodeKind::Folder),
            );
            Self {
                role,
                workspace_id,
                root_id,
                state: Arc::new(Mutex::new(State {
                    nodes,
                    documents: HashMap::new(),
                })),
            }
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, State> {
            self.state.lock().expect("state lock")
        }

        /// Insert a folder directly (test setup).
        fn add_folder(&self, parent: Uuid, name: &str) -> Uuid {
            let id = Uuid::new_v4();
            self.lock().nodes.insert(
                id,
                raw_node(id, self.workspace_id, Some(parent), name, NodeKind::Folder),
            );
            id
        }

        /// Insert a document directly with content (test setup).
        fn add_document(&self, parent: Uuid, name: &str, content: &str) -> Uuid {
            let id = Uuid::new_v4();
            let metrics = content::compute(content);
            let mut state = self.lock();
            state.nodes.insert(
                id,
                raw_node(
                    id,
                    self.workspace_id,
                    Some(parent),
                    name,
                    NodeKind::Document,
                ),
            );
            state.documents.insert(
                id,
                Document {
                    node_id: id,
                    workspace_id: self.workspace_id,
                    content_md: content.to_owned(),
                    content_sha256: metrics.content_sha256,
                    byte_len: metrics.byte_len as i32,
                    line_count: metrics.line_count as i32,
                    created_by: actor(),
                    updated_by: actor(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
            );
            id
        }

        fn mark_deleted(&self, id: Uuid) {
            if let Some(node) = self.lock().nodes.get_mut(&id) {
                node.deleted_at = Some(Utc::now());
                node.deleted_by = Some(actor());
            }
        }

        fn derive_path(state: &State, id: Uuid) -> Option<String> {
            let node = state.nodes.get(&id)?;
            match node.parent_id {
                None => Some("/".to_owned()),
                Some(parent) => {
                    let parent_path = Self::derive_path(state, parent)?;
                    Some(super::join_path(&parent_path, &node.name))
                }
            }
        }

        fn live_children(state: &State, parent: Uuid) -> Vec<Node> {
            let mut children: Vec<Node> = state
                .nodes
                .values()
                .filter(|n| n.parent_id == Some(parent) && n.deleted_at.is_none())
                .cloned()
                .collect();
            children
                .sort_by(|a, b| (a.sort_order, &a.name, a.id).cmp(&(b.sort_order, &b.name, b.id)));
            children
        }

        fn relative_depth(state: &State, id: Uuid) -> usize {
            Self::live_children(state, id)
                .into_iter()
                .map(|child| 1 + Self::relative_depth(state, child.id))
                .max()
                .unwrap_or(0)
        }

        fn subtree_count(state: &State, id: Uuid) -> usize {
            1 + Self::live_children(state, id)
                .into_iter()
                .map(|child| Self::subtree_count(state, child.id))
                .sum::<usize>()
        }

        fn is_descendant(state: &State, ancestor: Uuid, candidate: Uuid) -> bool {
            if ancestor == candidate {
                return true;
            }
            Self::live_children(state, ancestor)
                .into_iter()
                .any(|child| Self::is_descendant(state, child.id, candidate))
        }
    }

    fn raw_node(
        id: Uuid,
        workspace_id: Uuid,
        parent_id: Option<Uuid>,
        name: &str,
        kind: NodeKind,
    ) -> Node {
        Node {
            id,
            workspace_id,
            parent_id,
            name: name.to_owned(),
            kind,
            sort_order: 0,
            created_by: actor(),
            updated_by: actor(),
            deleted_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    impl FilesStore for MemStore {
        async fn role_for(&self, _ws: Uuid, _account: Uuid) -> CoreResult<Option<Role>> {
            Ok(self.role)
        }

        async fn root_node(&self, _ws: Uuid) -> CoreResult<Node> {
            Ok(self.lock().nodes.get(&self.root_id).cloned().expect("root"))
        }

        async fn find_node(&self, _ws: Uuid, id: Uuid) -> CoreResult<Option<Node>> {
            Ok(self
                .lock()
                .nodes
                .get(&id)
                .filter(|n| n.deleted_at.is_none())
                .cloned())
        }

        async fn find_deleted_node(&self, _ws: Uuid, id: Uuid) -> CoreResult<Option<Node>> {
            Ok(self
                .lock()
                .nodes
                .get(&id)
                .filter(|n| n.deleted_at.is_some())
                .cloned())
        }

        async fn node_path(&self, _ws: Uuid, id: Uuid) -> CoreResult<Option<String>> {
            let state = self.lock();
            Ok(Self::derive_path(&state, id))
        }

        async fn resolve_path(&self, _ws: Uuid, path: &str) -> CoreResult<Option<Uuid>> {
            let state = self.lock();
            let trimmed = path.trim();
            let mut current = Some(self.root_id);
            if trimmed.is_empty() || trimmed == "/" {
                return Ok(current);
            }
            for segment in trimmed.split('/').filter(|s| !s.is_empty()) {
                let Some(parent) = current else {
                    return Ok(None);
                };
                current = Self::live_children(&state, parent)
                    .into_iter()
                    .find(|n| n.name == segment)
                    .map(|n| n.id);
            }
            Ok(current)
        }

        async fn has_children(&self, _ws: Uuid, id: Uuid) -> CoreResult<bool> {
            Ok(!Self::live_children(&self.lock(), id).is_empty())
        }

        async fn count_live_children(&self, _ws: Uuid, parent: Uuid) -> CoreResult<usize> {
            Ok(Self::live_children(&self.lock(), parent).len())
        }

        async fn find_live_child_by_name(
            &self,
            _ws: Uuid,
            parent: Uuid,
            name: &str,
        ) -> CoreResult<Option<Node>> {
            Ok(Self::live_children(&self.lock(), parent)
                .into_iter()
                .find(|n| n.name == name))
        }

        async fn count_live_nodes(&self, _ws: Uuid) -> CoreResult<usize> {
            Ok(self
                .lock()
                .nodes
                .values()
                .filter(|n| n.deleted_at.is_none())
                .count())
        }

        async fn count_live_documents(&self, _ws: Uuid) -> CoreResult<usize> {
            let state = self.lock();
            Ok(state
                .nodes
                .values()
                .filter(|n| n.deleted_at.is_none() && n.kind == NodeKind::Document)
                .count())
        }

        async fn sum_live_document_bytes(&self, _ws: Uuid) -> CoreResult<usize> {
            let state = self.lock();
            Ok(state
                .documents
                .values()
                .filter(|d| {
                    state
                        .nodes
                        .get(&d.node_id)
                        .map(|n| n.deleted_at.is_none())
                        .unwrap_or(false)
                })
                .map(|d| d.byte_len.max(0) as usize)
                .sum())
        }

        async fn find_document(&self, _ws: Uuid, id: Uuid) -> CoreResult<Option<(Node, Document)>> {
            let state = self.lock();
            let Some(node) = state
                .nodes
                .get(&id)
                .filter(|n| n.deleted_at.is_none())
                .cloned()
            else {
                return Ok(None);
            };
            if node.kind != NodeKind::Document {
                return Ok(None);
            }
            Ok(state.documents.get(&id).cloned().map(|doc| (node, doc)))
        }

        async fn paged_children(
            &self,
            _ws: Uuid,
            parent: Uuid,
            limit: i64,
            cursor: Option<&ChildrenCursor>,
        ) -> CoreResult<(Vec<Node>, bool)> {
            let all = Self::live_children(&self.lock(), parent);
            let start = match cursor {
                None => 0,
                Some(c) => all
                    .iter()
                    .position(|n| {
                        (n.sort_order, n.name.as_str(), n.id)
                            > (c.sort_order, c.name.as_str(), c.id)
                    })
                    .unwrap_or(all.len()),
            };
            let window: Vec<Node> = all
                .iter()
                .skip(start)
                .take(limit as usize)
                .cloned()
                .collect();
            let has_more = start + window.len() < all.len();
            Ok((window, has_more))
        }

        async fn subtree_relative_depth(&self, _ws: Uuid, id: Uuid) -> CoreResult<usize> {
            Ok(Self::relative_depth(&self.lock(), id))
        }

        async fn subtree_live_count(&self, _ws: Uuid, id: Uuid) -> CoreResult<usize> {
            Ok(Self::subtree_count(&self.lock(), id))
        }

        async fn is_self_or_descendant(
            &self,
            _ws: Uuid,
            node_id: Uuid,
            candidate: Uuid,
        ) -> CoreResult<bool> {
            Ok(Self::is_descendant(&self.lock(), node_id, candidate))
        }

        async fn has_deleted_ancestor(&self, _ws: Uuid, id: Uuid) -> CoreResult<bool> {
            let state = self.lock();
            let mut current = state.nodes.get(&id).and_then(|n| n.parent_id);
            while let Some(parent) = current {
                let Some(node) = state.nodes.get(&parent) else {
                    break;
                };
                if node.deleted_at.is_some() {
                    return Ok(true);
                }
                current = node.parent_id;
            }
            Ok(false)
        }

        async fn insert_folder(
            &self,
            _ws: Uuid,
            command: &CreateFolder,
            created_by: Uuid,
        ) -> CoreResult<Node> {
            let id = Uuid::new_v4();
            let mut node = raw_node(
                id,
                self.workspace_id,
                Some(command.parent_node_id),
                &command.name,
                NodeKind::Folder,
            );
            node.created_by = created_by;
            node.updated_by = created_by;
            self.lock().nodes.insert(id, node.clone());
            Ok(node)
        }

        async fn insert_document(
            &self,
            _ws: Uuid,
            parent_node_id: Uuid,
            name: &str,
            content: &StoredContent,
            created_by: Uuid,
        ) -> CoreResult<(Node, Document)> {
            let id = Uuid::new_v4();
            let mut node = raw_node(
                id,
                self.workspace_id,
                Some(parent_node_id),
                name,
                NodeKind::Document,
            );
            node.created_by = created_by;
            node.updated_by = created_by;
            let doc = Document {
                node_id: id,
                workspace_id: self.workspace_id,
                content_md: content.content_md.clone(),
                content_sha256: content.content_sha256.clone(),
                byte_len: content.byte_len,
                line_count: content.line_count,
                created_by,
                updated_by: created_by,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            let mut state = self.lock();
            state.nodes.insert(id, node.clone());
            state.documents.insert(id, doc.clone());
            Ok((node, doc))
        }

        async fn save_document_content(
            &self,
            _ws: Uuid,
            node_id: Uuid,
            content: &StoredContent,
            updated_by: Uuid,
        ) -> CoreResult<(Node, Document)> {
            let mut state = self.lock();
            let node = state.nodes.get_mut(&node_id).expect("node");
            node.updated_by = updated_by;
            node.updated_at = Utc::now();
            let node = node.clone();
            let doc = state.documents.get_mut(&node_id).expect("doc");
            doc.content_md = content.content_md.clone();
            doc.content_sha256 = content.content_sha256.clone();
            doc.byte_len = content.byte_len;
            doc.line_count = content.line_count;
            doc.updated_by = updated_by;
            doc.updated_at = Utc::now();
            Ok((node, doc.clone()))
        }

        async fn move_node(
            &self,
            _ws: Uuid,
            command: &MoveNode,
            updated_by: Uuid,
        ) -> CoreResult<Node> {
            let mut state = self.lock();
            let node = state.nodes.get_mut(&command.node_id).expect("node");
            node.parent_id = Some(command.new_parent_node_id);
            if let Some(ref name) = command.new_name {
                node.name = name.clone();
            }
            node.updated_by = updated_by;
            node.updated_at = Utc::now();
            Ok(node.clone())
        }

        async fn update_node_metadata(
            &self,
            _ws: Uuid,
            node_id: Uuid,
            new_name: Option<&str>,
            new_sort_order: Option<i32>,
            updated_by: Uuid,
        ) -> CoreResult<Node> {
            let mut state = self.lock();
            let node = state.nodes.get_mut(&node_id).expect("node");
            if let Some(name) = new_name {
                node.name = name.to_owned();
            }
            if let Some(order) = new_sort_order {
                node.sort_order = order;
            }
            node.updated_by = updated_by;
            node.updated_at = Utc::now();
            Ok(node.clone())
        }

        async fn soft_delete_node(
            &self,
            _ws: Uuid,
            node_id: Uuid,
            deleted_by: Uuid,
        ) -> CoreResult<()> {
            // Soft-delete the node and its live subtree.
            let ids = {
                let state = self.lock();
                let mut stack = vec![node_id];
                let mut all = Vec::new();
                while let Some(id) = stack.pop() {
                    all.push(id);
                    for child in Self::live_children(&state, id) {
                        stack.push(child.id);
                    }
                }
                all
            };
            let mut state = self.lock();
            for id in ids {
                if let Some(node) = state.nodes.get_mut(&id) {
                    node.deleted_at = Some(Utc::now());
                    node.deleted_by = Some(deleted_by);
                }
            }
            Ok(())
        }

        async fn restore_node(
            &self,
            _ws: Uuid,
            node_id: Uuid,
            restored_by: Uuid,
        ) -> CoreResult<Node> {
            let mut state = self.lock();
            let node = state.nodes.get_mut(&node_id).expect("node");
            node.deleted_at = None;
            node.deleted_by = None;
            node.updated_by = restored_by;
            node.updated_at = Utc::now();
            Ok(node.clone())
        }
    }

    fn service(role: Option<Role>) -> (FilesService<MemStore>, MemStore) {
        let store = MemStore::new(role);
        (FilesService::new(store.clone()), store)
    }

    // --- authorization wiring ---

    #[tokio::test]
    async fn no_role_is_not_found() {
        let (svc, store) = service(None);
        let err = svc.root(actor(), store.workspace_id).await.unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[tokio::test]
    async fn viewer_cannot_mkdir() {
        let (svc, store) = service(Some(Role::Viewer));
        let err = svc
            .create_folder(
                actor(),
                store.workspace_id,
                CreateFolder {
                    parent_node_id: store.root_id,
                    name: "notes".to_owned(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden(_)));
    }

    // --- mkdir / touch ---

    #[tokio::test]
    async fn editor_can_mkdir_and_path_is_derived() {
        let (svc, store) = service(Some(Role::Editor));
        let view = svc
            .create_folder(
                actor(),
                store.workspace_id,
                CreateFolder {
                    parent_node_id: store.root_id,
                    name: "notes".to_owned(),
                },
            )
            .await
            .unwrap();
        assert_eq!(view.path, "/notes");
        assert_eq!(view.node.kind, NodeKind::Folder);
    }

    #[tokio::test]
    async fn mkdir_name_conflict_is_conflict() {
        let (svc, store) = service(Some(Role::Editor));
        store.add_folder(store.root_id, "notes");
        let err = svc
            .create_folder(
                actor(),
                store.workspace_id,
                CreateFolder {
                    parent_node_id: store.root_id,
                    name: "notes".to_owned(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn mkdir_rejects_depth_over_five() {
        let (svc, store) = service(Some(Role::Editor));
        // root/a/b/c/d/e is depth 5; creating under e would be depth 6.
        let mut parent = store.root_id;
        for name in ["a", "b", "c", "d", "e"] {
            parent = store.add_folder(parent, name);
        }
        let err = svc
            .create_folder(
                actor(),
                store.workspace_id,
                CreateFolder {
                    parent_node_id: parent,
                    name: "f".to_owned(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn touch_creates_empty_document() {
        let (svc, store) = service(Some(Role::Editor));
        let view = svc
            .create_document(
                actor(),
                store.workspace_id,
                CreateDocument {
                    parent_node_id: store.root_id,
                    name: "note.md".to_owned(),
                },
            )
            .await
            .unwrap();
        assert_eq!(view.node.path, "/note.md");
        assert_eq!(view.document.byte_len, 0);
        assert_eq!(view.document.line_count, 0);
    }

    // --- read ---

    #[tokio::test]
    async fn read_unchanged_when_hash_matches() {
        let (svc, store) = service(Some(Role::Viewer));
        let id = store.add_document(store.root_id, "n.md", "# Note\n");
        let (_, doc) = store
            .find_document(store.workspace_id, id)
            .await
            .unwrap()
            .unwrap();
        let result = svc
            .read_document(
                actor(),
                store.workspace_id,
                ReadDocument {
                    node_id: id,
                    start_line: None,
                    max_lines: None,
                    max_bytes: None,
                    if_none_match_sha256: Some(doc.content_sha256.clone()),
                },
            )
            .await
            .unwrap();
        assert!(result.unchanged());
        assert!(result.content.is_none());
    }

    #[tokio::test]
    async fn read_truncates_by_max_lines() {
        let (svc, store) = service(Some(Role::Viewer));
        let id = store.add_document(store.root_id, "n.md", "l1\nl2\nl3\nl4\n");
        let result = svc
            .read_document(
                actor(),
                store.workspace_id,
                ReadDocument {
                    node_id: id,
                    start_line: Some(1),
                    max_lines: Some(2),
                    max_bytes: None,
                    if_none_match_sha256: None,
                },
            )
            .await
            .unwrap();
        let content = result.content.unwrap();
        assert_eq!(content.returned_lines, 2);
        assert!(content.truncated);
        assert_eq!(content.next_start_line, Some(3));
    }

    // --- write ---

    #[tokio::test]
    async fn write_existing_replaces_content() {
        let (svc, store) = service(Some(Role::Editor));
        let id = store.add_document(store.root_id, "n.md", "old");
        let view = svc
            .write_document(
                actor(),
                store.workspace_id,
                WriteDocument {
                    target: WriteTarget::Existing { node_id: id },
                    content_md: "new content\n".to_owned(),
                    expected_sha256: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(view.document.content_md, "new content\n");
        assert_eq!(view.document.line_count, 1);
    }

    #[tokio::test]
    async fn write_create_makes_missing_document() {
        let (svc, store) = service(Some(Role::Editor));
        let view = svc
            .write_document(
                actor(),
                store.workspace_id,
                WriteDocument {
                    target: WriteTarget::Create {
                        parent_node_id: store.root_id,
                        name: "fresh.md".to_owned(),
                    },
                    content_md: "# Fresh\n".to_owned(),
                    expected_sha256: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(view.node.path, "/fresh.md");
        assert_eq!(view.document.content_md, "# Fresh\n");
    }

    #[tokio::test]
    async fn write_existing_missing_is_not_found() {
        let (svc, store) = service(Some(Role::Editor));
        let err = svc
            .write_document(
                actor(),
                store.workspace_id,
                WriteDocument {
                    target: WriteTarget::Existing {
                        node_id: Uuid::new_v4(),
                    },
                    content_md: "x".to_owned(),
                    expected_sha256: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[tokio::test]
    async fn write_expected_sha_mismatch_is_conflict() {
        let (svc, store) = service(Some(Role::Editor));
        let id = store.add_document(store.root_id, "n.md", "old");
        let err = svc
            .write_document(
                actor(),
                store.workspace_id,
                WriteDocument {
                    target: WriteTarget::Existing { node_id: id },
                    content_md: "new".to_owned(),
                    expected_sha256: Some("deadbeef".to_owned()),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn write_rejects_too_many_lines() {
        let (svc, store) = service(Some(Role::Editor));
        let id = store.add_document(store.root_id, "n.md", "x");
        let big = "a\n".repeat(limits::DOCUMENT_MAX_LINES + 1);
        let err = svc
            .write_document(
                actor(),
                store.workspace_id,
                WriteDocument {
                    target: WriteTarget::Existing { node_id: id },
                    content_md: big,
                    expected_sha256: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    // --- patch (integration through the service) ---

    #[tokio::test]
    async fn patch_applies_and_returns_previous_sha() {
        let (svc, store) = service(Some(Role::Editor));
        let id = store.add_document(store.root_id, "n.md", "hello world\n");
        let (_, before) = store
            .find_document(store.workspace_id, id)
            .await
            .unwrap()
            .unwrap();
        let result = svc
            .patch_document(
                actor(),
                store.workspace_id,
                PatchDocument {
                    node_id: id,
                    edits: vec![Edit {
                        old_text: "world".to_owned(),
                        new_text: "there".to_owned(),
                    }],
                    expected_sha256: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(result.document.content_md, "hello there\n");
        assert_eq!(result.previous_sha256, before.content_sha256);
        assert_eq!(result.edits_applied, 1);
    }

    #[tokio::test]
    async fn patch_no_match_is_conflict() {
        let (svc, store) = service(Some(Role::Editor));
        let id = store.add_document(store.root_id, "n.md", "hello\n");
        let err = svc
            .patch_document(
                actor(),
                store.workspace_id,
                PatchDocument {
                    node_id: id,
                    edits: vec![Edit {
                        old_text: "missing".to_owned(),
                        new_text: "x".to_owned(),
                    }],
                    expected_sha256: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn patch_empty_edits_is_invalid_input() {
        let (svc, store) = service(Some(Role::Editor));
        let id = store.add_document(store.root_id, "n.md", "hello\n");
        let err = svc
            .patch_document(
                actor(),
                store.workspace_id,
                PatchDocument {
                    node_id: id,
                    edits: Vec::new(),
                    expected_sha256: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn patch_expected_sha_checked_before_matching() {
        let (svc, store) = service(Some(Role::Editor));
        let id = store.add_document(store.root_id, "n.md", "hello\n");
        let err = svc
            .patch_document(
                actor(),
                store.workspace_id,
                PatchDocument {
                    node_id: id,
                    edits: vec![Edit {
                        old_text: "hello".to_owned(),
                        new_text: "hi".to_owned(),
                    }],
                    expected_sha256: Some("stale".to_owned()),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    // --- mv ---

    #[tokio::test]
    async fn mv_root_is_forbidden() {
        let (svc, store) = service(Some(Role::Editor));
        let dest = store.add_folder(store.root_id, "dest");
        let err = svc
            .move_node(
                actor(),
                store.workspace_id,
                MoveNode {
                    node_id: store.root_id,
                    new_parent_node_id: dest,
                    new_name: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn mv_into_self_is_conflict() {
        let (svc, store) = service(Some(Role::Editor));
        let folder = store.add_folder(store.root_id, "folder");
        let err = svc
            .move_node(
                actor(),
                store.workspace_id,
                MoveNode {
                    node_id: folder,
                    new_parent_node_id: folder,
                    new_name: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn mv_into_descendant_is_conflict() {
        let (svc, store) = service(Some(Role::Editor));
        let parent = store.add_folder(store.root_id, "parent");
        let child = store.add_folder(parent, "child");
        let err = svc
            .move_node(
                actor(),
                store.workspace_id,
                MoveNode {
                    node_id: parent,
                    new_parent_node_id: child,
                    new_name: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn mv_destination_name_conflict_is_conflict() {
        let (svc, store) = service(Some(Role::Editor));
        let src_parent = store.add_folder(store.root_id, "src");
        let moving = store.add_document(src_parent, "note.md", "x");
        let dest = store.add_folder(store.root_id, "dest");
        store.add_document(dest, "note.md", "y");
        let err = svc
            .move_node(
                actor(),
                store.workspace_id,
                MoveNode {
                    node_id: moving,
                    new_parent_node_id: dest,
                    new_name: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn mv_same_path_is_noop_success() {
        let (svc, store) = service(Some(Role::Editor));
        let folder = store.add_folder(store.root_id, "folder");
        let doc = store.add_document(folder, "note.md", "x");
        let view = svc
            .move_node(
                actor(),
                store.workspace_id,
                MoveNode {
                    node_id: doc,
                    new_parent_node_id: folder,
                    new_name: Some("note.md".to_owned()),
                },
            )
            .await
            .unwrap();
        assert_eq!(view.path, "/folder/note.md");
    }

    #[tokio::test]
    async fn mv_into_full_destination_is_conflict() {
        let (svc, store) = service(Some(Role::Editor));
        let src_parent = store.add_folder(store.root_id, "src");
        let moving = store.add_document(src_parent, "note.md", "x");
        let dest = store.add_folder(store.root_id, "dest");
        // Fill the destination to the fanout cap.
        for i in 0..limits::FOLDER_MAX_CHILDREN {
            store.add_document(dest, &format!("f{i}.md"), "y");
        }
        let err = svc
            .move_node(
                actor(),
                store.workspace_id,
                MoveNode {
                    node_id: moving,
                    new_parent_node_id: dest,
                    new_name: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn mv_succeeds_and_derives_new_path() {
        let (svc, store) = service(Some(Role::Editor));
        let src = store.add_folder(store.root_id, "src");
        let doc = store.add_document(src, "note.md", "x");
        let dest = store.add_folder(store.root_id, "dest");
        let view = svc
            .move_node(
                actor(),
                store.workspace_id,
                MoveNode {
                    node_id: doc,
                    new_parent_node_id: dest,
                    new_name: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(view.path, "/dest/note.md");
    }

    // --- rm ---

    #[tokio::test]
    async fn rm_root_is_forbidden() {
        let (svc, store) = service(Some(Role::Editor));
        let err = svc
            .delete_node(
                actor(),
                store.workspace_id,
                DeleteNode {
                    node_id: store.root_id,
                    recursive: false,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn rm_folder_without_recursive_is_conflict() {
        let (svc, store) = service(Some(Role::Editor));
        let folder = store.add_folder(store.root_id, "folder");
        let err = svc
            .delete_node(
                actor(),
                store.workspace_id,
                DeleteNode {
                    node_id: folder,
                    recursive: false,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn rm_document_succeeds_without_recursive() {
        let (svc, store) = service(Some(Role::Editor));
        let doc = store.add_document(store.root_id, "n.md", "x");
        svc.delete_node(
            actor(),
            store.workspace_id,
            DeleteNode {
                node_id: doc,
                recursive: false,
            },
        )
        .await
        .unwrap();
        assert!(
            store
                .find_node(store.workspace_id, doc)
                .await
                .unwrap()
                .is_none()
        );
    }

    // --- restore ---

    #[tokio::test]
    async fn restore_rejects_when_ancestor_deleted() {
        let (svc, store) = service(Some(Role::Editor));
        let parent = store.add_folder(store.root_id, "parent");
        let child = store.add_document(parent, "note.md", "x");
        // Delete the child, then also delete the parent (ancestor still deleted).
        store.mark_deleted(child);
        store.mark_deleted(parent);
        let err = svc
            .restore_node(actor(), store.workspace_id, RestoreNode { node_id: child })
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn restore_succeeds_with_live_ancestor() {
        let (svc, store) = service(Some(Role::Editor));
        let parent = store.add_folder(store.root_id, "parent");
        let child = store.add_document(parent, "note.md", "x");
        store.mark_deleted(child);
        let view = svc
            .restore_node(actor(), store.workspace_id, RestoreNode { node_id: child })
            .await
            .unwrap();
        assert_eq!(view.path, "/parent/note.md");
        assert!(view.node.deleted_at.is_none());
    }

    #[tokio::test]
    async fn restore_rejects_sibling_name_conflict() {
        let (svc, store) = service(Some(Role::Editor));
        let parent = store.add_folder(store.root_id, "parent");
        let deleted = store.add_document(parent, "note.md", "x");
        store.mark_deleted(deleted);
        // A live sibling now occupies the same name.
        store.add_document(parent, "note.md", "y");
        let err = svc
            .restore_node(
                actor(),
                store.workspace_id,
                RestoreNode { node_id: deleted },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    // --- ls pagination ---

    #[tokio::test]
    async fn children_paginate_with_cursor() {
        let (svc, store) = service(Some(Role::Viewer));
        for i in 0..5 {
            store.add_document(store.root_id, &format!("f{i}.md"), "x");
        }
        let page1 = svc
            .children(
                actor(),
                store.workspace_id,
                store.root_id,
                ChildrenRequest {
                    limit: Some(2),
                    cursor: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(page1.items.len(), 2);
        assert!(page1.has_more);
        let cursor = page1.next_cursor.clone();
        assert!(cursor.is_some());

        let page2 = svc
            .children(
                actor(),
                store.workspace_id,
                store.root_id,
                ChildrenRequest {
                    limit: Some(2),
                    cursor,
                },
            )
            .await
            .unwrap();
        assert_eq!(page2.items.len(), 2);
        // No overlap between page 1 and page 2.
        let ids1: Vec<Uuid> = page1.items.iter().map(|n| n.node.id).collect();
        let ids2: Vec<Uuid> = page2.items.iter().map(|n| n.node.id).collect();
        assert!(ids1.iter().all(|id| !ids2.contains(id)));
    }

    // --- resolve_path ---

    #[tokio::test]
    async fn resolve_path_root_returns_root() {
        let (svc, store) = service(Some(Role::Viewer));
        let view = svc
            .resolve_path(actor(), store.workspace_id, "/")
            .await
            .unwrap();
        assert_eq!(view.node.id, store.root_id);
        assert_eq!(view.path, "/");
    }

    #[tokio::test]
    async fn resolve_path_nested_returns_node() {
        let (svc, store) = service(Some(Role::Viewer));
        let folder = store.add_folder(store.root_id, "projects");
        let doc = store.add_document(folder, "note.md", "x");
        let view = svc
            .resolve_path(actor(), store.workspace_id, "/projects/note.md")
            .await
            .unwrap();
        assert_eq!(view.node.id, doc);
        assert_eq!(view.path, "/projects/note.md");
    }

    #[tokio::test]
    async fn resolve_path_missing_is_not_found() {
        let (svc, store) = service(Some(Role::Viewer));
        let err = svc
            .resolve_path(actor(), store.workspace_id, "/nope/missing.md")
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[tokio::test]
    async fn resolve_path_requires_role() {
        let (svc, store) = service(None);
        let err = svc
            .resolve_path(actor(), store.workspace_id, "/")
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    // --- update_node (rename / reorder) ---

    #[tokio::test]
    async fn update_node_renames_and_derives_path() {
        let (svc, store) = service(Some(Role::Editor));
        let folder = store.add_folder(store.root_id, "old");
        let view = svc
            .update_node(
                actor(),
                store.workspace_id,
                folder,
                Some("new".to_owned()),
                None,
            )
            .await
            .unwrap();
        assert_eq!(view.node.name, "new");
        assert_eq!(view.path, "/new");
    }

    #[tokio::test]
    async fn update_node_sets_sort_order() {
        let (svc, store) = service(Some(Role::Editor));
        let folder = store.add_folder(store.root_id, "f");
        let view = svc
            .update_node(actor(), store.workspace_id, folder, None, Some(10))
            .await
            .unwrap();
        assert_eq!(view.node.sort_order, 10);
    }

    #[tokio::test]
    async fn update_node_rejects_root_rename() {
        let (svc, store) = service(Some(Role::Editor));
        let err = svc
            .update_node(
                actor(),
                store.workspace_id,
                store.root_id,
                Some("renamed".to_owned()),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn update_node_rejects_sibling_name_conflict() {
        let (svc, store) = service(Some(Role::Editor));
        store.add_folder(store.root_id, "taken");
        let other = store.add_folder(store.root_id, "other");
        let err = svc
            .update_node(
                actor(),
                store.workspace_id,
                other,
                Some("taken".to_owned()),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Conflict(_)));
    }

    #[tokio::test]
    async fn update_node_requires_a_field() {
        let (svc, store) = service(Some(Role::Editor));
        let folder = store.add_folder(store.root_id, "f");
        let err = svc
            .update_node(actor(), store.workspace_id, folder, None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn update_node_requires_editor() {
        let (svc, store) = service(Some(Role::Viewer));
        let folder = store.add_folder(store.root_id, "f");
        let err = svc
            .update_node(
                actor(),
                store.workspace_id,
                folder,
                Some("g".to_owned()),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden(_)));
    }
}
