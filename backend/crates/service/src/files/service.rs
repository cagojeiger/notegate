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
                    .save_document_content(
                        workspace_id,
                        node.id,
                        &stored,
                        command.expected_sha256.as_deref(),
                        caller_account_id,
                    )
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
            .save_document_content(
                workspace_id,
                node.id,
                &stored,
                command.expected_sha256.as_deref(),
                caller_account_id,
            )
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

    /// Load a live document, distinguishing a folder from a missing document.
    async fn load_document(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<(Node, Document)> {
        if let Some(document) = self.store.find_document(workspace_id, node_id).await? {
            return Ok(document);
        }

        if let Some(node) = self.store.find_node(workspace_id, node_id).await?
            && node.kind == NodeKind::Folder
        {
            return Err(ServiceError::InvalidInput(
                "target is a folder, not a document".to_owned(),
            ));
        }

        Err(ServiceError::NotFound("document not found".to_owned()))
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
mod tests;
