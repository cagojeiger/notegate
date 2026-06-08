use notegate_core::limits;
use notegate_model::NodeKind;
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::files::patch::{apply_edits, unified_diff};
use crate::files::validation;
use crate::files::{
    CreateDocument, CreateFolder, DeleteNode, DeleteResult, DocumentView, FileCommand, MoveNode,
    NodeView, PatchDocument, PatchResult, WriteDocument, WriteTarget, content,
};

use super::view::document_view_at_path;
use super::{FilesService, join_path, path_depth};

impl FilesService {
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
            document: None,
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
        validation::validate_workspace_document_count(documents, self.limits)?;

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
        Ok(document_view_at_path(node, path, document))
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
                    self.limits,
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
                validation::validate_workspace_document_count(documents, self.limits)?;
                let total = self.store.sum_live_document_bytes(workspace_id).await?;
                validation::validate_workspace_document_bytes(
                    total,
                    0,
                    metrics.byte_len,
                    self.limits,
                )?;

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
            self.limits,
        )?;

        let stored = metrics.into_stored(new_content);
        let save_guard = command
            .expected_sha256
            .as_deref()
            .unwrap_or(&previous_sha256);
        let (node, document) = self
            .store
            .save_document_content(
                workspace_id,
                node.id,
                &stored,
                Some(save_guard),
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
        if let Some(expected_parent_id) = command.expected_parent_id
            && node.parent_id != Some(expected_parent_id)
        {
            return Err(ServiceError::Conflict(
                "expected_parent_id does not match the node's current parent; refresh and retry"
                    .to_owned(),
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
            validation::validate_fanout(children, self.limits)?;
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

    /// Delete a node (`rm`). Requires `editor`.
    ///
    /// The node is hidden immediately by soft-deleting the live subtree. Public
    /// recovery is intentionally not part of the product contract; the returned
    /// `purge_after` is when an internal purge job may hard-delete the rows.
    pub async fn delete_node(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        command: DeleteNode,
    ) -> ServiceResult<DeleteResult> {
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

        let path = self.path_of(workspace_id, node.id).await?;
        let purge_after = self
            .store
            .soft_delete_node(workspace_id, node.id, caller_account_id)
            .await?;

        Ok(DeleteResult {
            node_id: node.id,
            path,
            purge_after,
        })
    }
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
