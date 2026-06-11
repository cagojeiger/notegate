use notegate_core::limits;
use notegate_model::NodeKind;
use serde_json::Value;
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::files::patch::{apply_edits, unified_diff};
use crate::files::validation;
use crate::files::{
    CreateFolder, CreateText, DeleteNode, DeleteResult, FileCommand, MoveNode, NodeView,
    PatchResult, PatchText, StoredContent, TextView, WriteTarget, WriteText, WriteTextBody,
    content,
};

use super::view::text_view_at_path;
use super::{FilesService, join_path, path_depth};

impl FilesService {
    /// Create a folder (`mkdir`). Requires write permission.
    pub async fn create_folder(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: CreateFolder,
    ) -> ServiceResult<NodeView> {
        self.authorize(space_id, caller_account_id, FileCommand::Mkdir)
            .await?;
        validation::validate_basename(&command.name, NodeKind::Folder)?;

        let parent_path = self
            .prepare_create(space_id, command.parent_node_id, &command.name)
            .await?;

        let node = self
            .store
            .insert_folder(space_id, &command, caller_account_id)
            .await?;
        let path = join_path(&parent_path, &node.name);
        Ok(NodeView {
            node,
            path,
            has_children: false,
            text: None,
        })
    }

    /// Create an empty text (`touch`). Requires write permission.
    pub async fn create_text(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: CreateText,
    ) -> ServiceResult<TextView> {
        self.authorize(space_id, caller_account_id, FileCommand::Touch)
            .await?;
        validation::validate_basename(&command.name, NodeKind::Text)?;

        let parent_path = self
            .prepare_create(space_id, command.parent_node_id, &command.name)
            .await?;

        // A text also consumes the live-text quota.
        let texts = self.store.count_live_texts(space_id).await?;
        validation::validate_space_text_count(texts, self.limits)?;

        let empty = content::compute("").into_stored_plain(String::new());
        let (node, text) = self
            .store
            .insert_text(
                space_id,
                command.parent_node_id,
                &command.name,
                &empty,
                caller_account_id,
            )
            .await?;
        let path = join_path(&parent_path, &node.name);
        Ok(text_view_at_path(node, path, text))
    }

    /// Replace a text's content (`write`/`save`). Requires write permission.
    ///
    /// [`WriteTarget::Existing`] replaces an existing text (the `create=false`
    /// case, and the resolved `create=true` case). [`WriteTarget::Create`] makes a
    /// new text, re-checking node/text/fanout/depth/name limits. Both
    /// enforce the per-text and space-total content caps; the existing
    /// case also honors the `expected_sha256` optimistic guard.
    pub async fn write_text(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: WriteText,
    ) -> ServiceResult<TextView> {
        self.authorize(space_id, caller_account_id, FileCommand::Write)
            .await?;

        let stored = stored_text_body(command.body)?;
        validation::validate_text_content(stored.byte_len as usize, stored.line_count as usize)?;

        match command.target {
            WriteTarget::Existing { node_id } => {
                let (node, text) = self.load_text(space_id, node_id).await?;
                check_expected_sha(command.expected_sha256.as_deref(), &text.content_sha256)?;

                let total = self.store.sum_live_text_bytes(space_id).await?;
                validation::validate_space_text_bytes(
                    total,
                    text.byte_len.max(0) as usize,
                    stored.byte_len as usize,
                    self.limits,
                )?;

                let (node, text) = self
                    .store
                    .save_text_content(
                        space_id,
                        node.id,
                        &stored,
                        command.expected_sha256.as_deref(),
                        caller_account_id,
                    )
                    .await?;
                self.text_view(space_id, node, text).await
            }
            WriteTarget::Create {
                parent_node_id,
                name,
            } => {
                // expected_sha256 cannot match a not-yet-existent text.
                if command.expected_sha256.is_some() {
                    return Err(ServiceError::Conflict(
                        "expected_sha256 was supplied but the text does not exist".to_owned(),
                    ));
                }
                validation::validate_basename(&name, NodeKind::Text)?;
                self.prepare_create(space_id, parent_node_id, &name).await?;

                // New-text quotas: live-text count and total byte budget.
                let texts = self.store.count_live_texts(space_id).await?;
                validation::validate_space_text_count(texts, self.limits)?;
                let total = self.store.sum_live_text_bytes(space_id).await?;
                validation::validate_space_text_bytes(
                    total,
                    0,
                    stored.byte_len as usize,
                    self.limits,
                )?;

                let (node, text) = self
                    .store
                    .insert_text(space_id, parent_node_id, &name, &stored, caller_account_id)
                    .await?;
                self.text_view(space_id, node, text).await
            }
        }
    }

    /// Apply exact targeted edits to a text (`patch`). Requires write permission.
    pub async fn patch_text(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: PatchText,
    ) -> ServiceResult<PatchResult> {
        self.authorize(space_id, caller_account_id, FileCommand::Patch)
            .await?;

        if command.edits.is_empty() {
            return Err(ServiceError::InvalidInput(
                "edits must not be empty".to_owned(),
            ));
        }

        let (node, text) = self.load_text(space_id, command.node_id).await?;
        let previous_sha256 = text.content_sha256.clone();

        // expected_sha256 is checked before any matching.
        check_expected_sha(command.expected_sha256.as_deref(), &previous_sha256)?;

        let plain_content = text.content.as_deref().ok_or_else(|| {
            ServiceError::InvalidInput("text content is not stored as plaintext".to_owned())
        })?;
        let new_content = apply_edits(plain_content, &command.edits)?;
        let diff = unified_diff(plain_content, &new_content);

        let metrics = content::compute(&new_content);
        validation::validate_text_content(metrics.byte_len, metrics.line_count)?;

        let total = self.store.sum_live_text_bytes(space_id).await?;
        validation::validate_space_text_bytes(
            total,
            text.byte_len.max(0) as usize,
            metrics.byte_len,
            self.limits,
        )?;

        let stored = metrics.into_stored_plain(new_content);
        let save_guard = command
            .expected_sha256
            .as_deref()
            .unwrap_or(&previous_sha256);
        let (node, text) = self
            .store
            .save_text_content(
                space_id,
                node.id,
                &stored,
                Some(save_guard),
                caller_account_id,
            )
            .await?;
        let view = self.text_view(space_id, node, text).await?;

        Ok(PatchResult {
            node: view.node,
            text: view.text,
            previous_sha256,
            edits_applied: command.edits.len(),
            diff,
        })
    }

    /// Read a node's metadata object. Requires read permission.
    pub async fn read_metadata(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<Value> {
        self.authorize(space_id, caller_account_id, FileCommand::Stat)
            .await?;
        Ok(self.load_node(space_id, node_id).await?.metadata)
    }

    /// Replace a node's metadata object. Requires write permission.
    pub async fn replace_metadata(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
        metadata: Value,
    ) -> ServiceResult<NodeView> {
        self.authorize(space_id, caller_account_id, FileCommand::Write)
            .await?;
        validation::validate_metadata(&metadata)?;

        let updated = self
            .store
            .replace_node_metadata(space_id, node_id, &metadata, caller_account_id)
            .await?;
        self.node_view(space_id, updated).await
    }

    /// Merge-patch a node's metadata object. Requires write permission.
    pub async fn patch_metadata(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
        patch: Value,
    ) -> ServiceResult<NodeView> {
        self.authorize(space_id, caller_account_id, FileCommand::Patch)
            .await?;
        if !patch.is_object() {
            return Err(ServiceError::InvalidInput(
                "metadata patch must be a JSON object".to_owned(),
            ));
        }

        let mut metadata = self.load_node(space_id, node_id).await?.metadata;
        apply_json_merge_patch(&mut metadata, patch);
        validation::validate_metadata(&metadata)?;

        let updated = self
            .store
            .replace_node_metadata(space_id, node_id, &metadata, caller_account_id)
            .await?;
        self.node_view(space_id, updated).await
    }

    /// Move or rename a node (`mv`). Requires write permission.
    pub async fn move_node(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: MoveNode,
    ) -> ServiceResult<NodeView> {
        self.authorize(space_id, caller_account_id, FileCommand::Mv)
            .await?;

        let node = self.load_node(space_id, command.node_id).await?;
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

        let dest_parent = self.load_node(space_id, command.new_parent_node_id).await?;
        if dest_parent.kind != NodeKind::Folder {
            return Err(ServiceError::Conflict(
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
            return self.node_view(space_id, node).await;
        }

        // Cannot move a node into itself or its own descendant.
        if self
            .store
            .is_self_or_descendant(space_id, command.node_id, command.new_parent_node_id)
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
            .find_live_child_by_name(space_id, command.new_parent_node_id, &final_name)
            .await?
            && conflict.id != node.id
        {
            return Err(ServiceError::Conflict(format!(
                "destination already has a node named '{final_name}'"
            )));
        }

        // Resulting subtree depth and path length.
        let dest_parent_path = self.path_of(space_id, command.new_parent_node_id).await?;
        let dest_parent_depth = path_depth(&dest_parent_path);
        let new_path = join_path(&dest_parent_path, &final_name);
        validation::validate_path_len(&new_path)?;
        let subtree_depth = self
            .store
            .subtree_relative_depth(space_id, command.node_id)
            .await?;
        validation::validate_depth(dest_parent_depth + 1 + subtree_depth)?;

        // Destination fanout (only when actually changing parent).
        if node.parent_id != Some(command.new_parent_node_id) {
            let children = self
                .store
                .count_live_children(space_id, command.new_parent_node_id)
                .await?;
            validation::validate_fanout(children, self.limits)?;
        }

        let moved = self
            .store
            .move_node(space_id, &command, caller_account_id)
            .await?;
        self.node_view(space_id, moved).await
    }

    /// Update a node's in-place metadata: rename and/or reorder (`PATCH`).
    /// Requires write permission. The node keeps its parent. Renaming the root is
    /// rejected; a rename validates the new basename and sibling-name uniqueness.
    /// At least one of `new_name`/`new_sort_order` must be present.
    pub async fn update_node(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
        new_name: Option<String>,
        new_sort_order: Option<i32>,
    ) -> ServiceResult<NodeView> {
        self.authorize(space_id, caller_account_id, FileCommand::Mv)
            .await?;

        if new_name.is_none() && new_sort_order.is_none() {
            return Err(ServiceError::InvalidInput(
                "provide name and/or sort_order to update".to_owned(),
            ));
        }

        let node = self.load_node(space_id, node_id).await?;

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
                    .find_live_child_by_name(space_id, parent_id, name)
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
                space_id,
                node_id,
                new_name.as_deref(),
                new_sort_order,
                caller_account_id,
            )
            .await?;
        self.node_view(space_id, updated).await
    }

    /// Delete a node (`rm`). Requires write permission.
    ///
    /// The node is hidden immediately by soft-deleting the live subtree. Public
    /// recovery is intentionally not part of the product contract; the returned
    /// `purge_after` is when an internal purge job may hard-delete the rows.
    pub async fn delete_node(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: DeleteNode,
    ) -> ServiceResult<DeleteResult> {
        self.authorize(space_id, caller_account_id, FileCommand::Rm)
            .await?;

        let node = self.load_node(space_id, command.node_id).await?;
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
            let subtree = self.store.subtree_live_count(space_id, node.id).await?;
            if subtree > limits::SUBTREE_DELETE_MAX_NODES {
                return Err(ServiceError::Conflict(format!(
                    "subtree of {subtree} nodes exceeds the synchronous delete limit of {}; narrow the operation",
                    limits::SUBTREE_DELETE_MAX_NODES
                )));
            }
        }

        let path = self.path_of(space_id, node.id).await?;
        let purge_after = self
            .store
            .soft_delete_node(space_id, node.id, caller_account_id)
            .await?;

        Ok(DeleteResult {
            node_id: node.id,
            path,
            purge_after,
        })
    }
}

fn stored_text_body(body: WriteTextBody) -> ServiceResult<StoredContent> {
    match body {
        WriteTextBody::Plain(content) => {
            let metrics = content::compute(&content);
            Ok(metrics.into_stored_plain(content))
        }
        WriteTextBody::Encrypted(payload) => content::compute_encrypted(payload),
    }
}

fn apply_json_merge_patch(target: &mut Value, patch: Value) {
    match (target, patch) {
        (Value::Object(target), Value::Object(patch)) => {
            for (key, value) in patch {
                if value.is_null() {
                    target.remove(&key);
                } else {
                    apply_json_merge_patch(target.entry(key).or_insert(Value::Null), value);
                }
            }
        }
        (target, patch) => *target = patch,
    }
}

/// Compare an optional `expected_sha256` to the current hash; conflict on mismatch.
fn check_expected_sha(expected: Option<&str>, current: &str) -> ServiceResult<()> {
    if let Some(expected) = expected
        && expected != current
    {
        return Err(ServiceError::Conflict(
            "expected_sha256 does not match the current text; read it again".to_owned(),
        ));
    }
    Ok(())
}
