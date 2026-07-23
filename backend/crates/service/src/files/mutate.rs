use notegate_core::limits;
use notegate_db::{MetadataMutationKind, TextMutationKind};
use notegate_model::NodeKind;
use serde_json::Value;
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::files::format::validate_structured_text;
use crate::files::patch::{AppliedText, apply_edits, apply_line_edits, unified_diff};
use crate::files::validation;
use crate::files::{
    AppendText, BeginObjectUpload, CopyNode, CopyResult, CreateFolder, CreateText, DeleteNode,
    DeleteResult, EditText, FileCommand, MoveNode, NodeView, PatchResult, PatchText,
    PendingObjectUpload, StoredContent, TextView, WriteTarget, WriteText, WriteTextBody, content,
};

use super::view::{file_view_at_path, text_view_at_path};
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
            file: None,
        })
    }

    /// Create a folder and any missing ancestor folders (`mkdir -p`). Each
    /// path segment is resolved in turn: an existing folder is descended
    /// into, an existing non-folder is a conflict, and a missing segment is
    /// created. Returns the final folder's view and the paths that were
    /// actually created (already-existing ancestors are not included).
    pub async fn create_folder_recursive(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        path: &str,
    ) -> ServiceResult<(NodeView, Vec<String>)> {
        let mut current = self.resolve_path(caller_account_id, space_id, "/").await?;
        let mut current_path = "/".to_owned();
        let mut created_paths = Vec::new();

        for segment in path.split('/').filter(|segment| !segment.is_empty()) {
            let next_path = if current_path == "/" {
                format!("/{segment}")
            } else {
                format!("{current_path}/{segment}")
            };

            match self
                .resolve_path(caller_account_id, space_id, &next_path)
                .await
            {
                Ok(existing) if existing.node.kind == NodeKind::Folder => {
                    current = existing;
                    current_path = next_path;
                }
                Ok(_existing) => {
                    return Err(ServiceError::Conflict(format!(
                        "path component '{next_path}' exists and is not a folder"
                    )));
                }
                Err(ServiceError::NotFound(_)) => {
                    let created = self
                        .create_folder(
                            caller_account_id,
                            space_id,
                            CreateFolder {
                                parent_node_id: current.node.id,
                                name: segment.to_owned(),
                            },
                        )
                        .await?;
                    created_paths.push(created.path.clone());
                    current = created;
                    current_path = next_path;
                }
                Err(error) => return Err(error),
            }
        }

        Ok((current, created_paths))
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

    pub async fn prepare_object_upload(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: &BeginObjectUpload,
    ) -> ServiceResult<()> {
        self.authorize(space_id, caller_account_id, FileCommand::Write)
            .await?;
        validation::validate_basename(&command.name, NodeKind::File)?;
        validation::validate_object_file_bytes(command.byte_len)?;
        validate_file_encryption(
            command.encryption_mode,
            command.encryption_metadata.as_ref(),
        )?;
        if command.media_type.is_empty()
            || command.media_type.len() > 255
            || !command
                .media_type
                .bytes()
                .all(|byte| (0x20..0x7f).contains(&byte))
        {
            return Err(ServiceError::InvalidInput("invalid media_type".to_owned()));
        }
        self.prepare_create(space_id, command.parent_node_id, &command.name)
            .await?;
        Ok(())
    }

    pub async fn record_object_upload(
        &self,
        upload_id: Uuid,
        object_key: &str,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: &BeginObjectUpload,
    ) -> ServiceResult<PendingObjectUpload> {
        let registration = notegate_model::files::ObjectUploadRegistration {
            id: upload_id,
            object_key: object_key.to_owned(),
            upload_mode: notegate_model::files::ObjectUploadMode::Single,
            multipart_upload_id: None,
            multipart_part_size: None,
        };
        self.record_registered_object_upload(&registration, caller_account_id, space_id, command)
            .await
    }

    pub async fn record_registered_object_upload(
        &self,
        registration: &notegate_model::files::ObjectUploadRegistration,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: &BeginObjectUpload,
    ) -> ServiceResult<PendingObjectUpload> {
        Ok(self
            .store
            .insert_registered_object_upload(registration, space_id, caller_account_id, command)
            .await?)
    }

    pub async fn object_upload(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        upload_id: Uuid,
    ) -> ServiceResult<PendingObjectUpload> {
        self.authorize(space_id, caller_account_id, FileCommand::Write)
            .await?;
        self.store
            .object_upload(upload_id, space_id, caller_account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("file upload not found".to_owned()))
    }

    pub async fn object_upload_by_id(
        &self,
        caller_account_id: Uuid,
        upload_id: Uuid,
    ) -> ServiceResult<PendingObjectUpload> {
        let upload = self
            .store
            .object_upload_for_caller(upload_id, caller_account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("file upload not found".to_owned()))?;
        self.authorize(upload.space_id, caller_account_id, FileCommand::Write)
            .await?;
        Ok(upload)
    }

    pub async fn touch_object_upload(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        upload_id: Uuid,
    ) -> ServiceResult<PendingObjectUpload> {
        let upload = self
            .object_upload(caller_account_id, space_id, upload_id)
            .await?;
        if upload.node_id.is_some() {
            return Ok(upload);
        }
        if !self
            .store
            .touch_object_upload(upload_id, space_id, caller_account_id)
            .await?
        {
            if let Some(attached) = self
                .store
                .object_upload(upload_id, space_id, caller_account_id)
                .await?
                && attached.node_id.is_some()
            {
                return Ok(attached);
            }
            return Err(ServiceError::Conflict(
                "file upload is no longer active".to_owned(),
            ));
        }
        Ok(upload)
    }

    pub async fn cancel_object_upload(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        upload_id: Uuid,
    ) -> ServiceResult<()> {
        let upload = self
            .object_upload(caller_account_id, space_id, upload_id)
            .await?;
        if upload.node_id.is_some() {
            return Err(ServiceError::Conflict(
                "attached file upload cannot be aborted".to_owned(),
            ));
        }
        if !self
            .store
            .request_object_upload_expiry(upload_id, space_id, caller_account_id)
            .await?
        {
            return Err(ServiceError::Conflict(
                "file upload is no longer active".to_owned(),
            ));
        }
        Ok(())
    }

    pub async fn complete_object_upload(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        upload_id: Uuid,
        detected_media_type: Option<&str>,
    ) -> ServiceResult<crate::files::FileView> {
        self.authorize(space_id, caller_account_id, FileCommand::Write)
            .await?;
        let (node, file) = self
            .store
            .attach_object_upload(upload_id, space_id, caller_account_id, detected_media_type)
            .await?;
        let path = self.path_of(space_id, node.id).await?;
        Ok(file_view_at_path(node, path, file))
    }

    pub async fn record_detected_file_media_type(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
        detected_media_type: &str,
    ) -> ServiceResult<()> {
        self.authorize(space_id, caller_account_id, FileCommand::Read)
            .await?;
        self.store
            .set_detected_file_media_type(space_id, node_id, detected_media_type)
            .await?;
        Ok(())
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
                validate_stored_text_format(&node.name, &stored)?;

                let (node, text) = self
                    .store
                    .save_text_content(
                        space_id,
                        node.id,
                        &stored,
                        command.expected_sha256.as_deref(),
                        caller_account_id,
                        TextMutationKind::Write,
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
                validate_stored_text_format(&name, &stored)?;
                self.prepare_create(space_id, parent_node_id, &name).await?;

                let (node, text) = self
                    .store
                    .insert_text(space_id, parent_node_id, &name, &stored, caller_account_id)
                    .await?;
                self.text_view(space_id, node, text).await
            }
        }
    }

    /// Append plain content to a text (`>>`). Requires write permission.
    pub async fn append_text(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: AppendText,
    ) -> ServiceResult<TextView> {
        self.authorize(space_id, caller_account_id, FileCommand::Append)
            .await?;

        match command.target {
            WriteTarget::Existing { node_id } => {
                let (node, text) = self.load_text(space_id, node_id).await?;
                let previous_sha256 = text.content_sha256.clone();
                check_expected_sha(command.expected_sha256.as_deref(), &previous_sha256)?;

                let existing = text.content.as_deref().ok_or_else(|| {
                    ServiceError::InvalidInput("text content is not stored as plaintext".to_owned())
                })?;
                let mut content = existing.to_owned();
                if command.ensure_newline && !content.is_empty() && !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str(&command.content);

                validate_structured_text(&node.name, &content)?;
                let metrics = content::compute(&content);
                validation::validate_text_content(metrics.byte_len, metrics.line_count)?;

                let stored = metrics.into_stored_plain(content);
                let (node, text) = self
                    .store
                    .save_text_content(
                        space_id,
                        node.id,
                        &stored,
                        Some(&previous_sha256),
                        caller_account_id,
                        TextMutationKind::Append,
                    )
                    .await?;
                self.text_view(space_id, node, text).await
            }
            WriteTarget::Create {
                parent_node_id,
                name,
            } => {
                if command.expected_sha256.is_some() {
                    return Err(ServiceError::Conflict(
                        "expected_sha256 was supplied but the text does not exist".to_owned(),
                    ));
                }
                self.write_text(
                    caller_account_id,
                    space_id,
                    WriteText {
                        target: WriteTarget::Create {
                            parent_node_id,
                            name,
                        },
                        body: WriteTextBody::Plain(command.content),
                        expected_sha256: None,
                    },
                )
                .await
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

        self.apply_text_mutation(
            caller_account_id,
            space_id,
            command.node_id,
            command.expected_sha256.as_deref(),
            TextMutationKind::Patch,
            |content| apply_edits(content, &command.edits).map_err(Into::into),
        )
        .await
    }

    /// Apply line-based edits to a plain text. Requires write permission.
    pub async fn edit_text(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: EditText,
    ) -> ServiceResult<PatchResult> {
        self.authorize(space_id, caller_account_id, FileCommand::Edit)
            .await?;

        if command.edits.is_empty() {
            return Err(ServiceError::InvalidInput(
                "edits must not be empty".to_owned(),
            ));
        }

        self.apply_text_mutation(
            caller_account_id,
            space_id,
            command.node_id,
            command.expected_sha256.as_deref(),
            TextMutationKind::Edit,
            |content| apply_line_edits(content, &command.edits).map_err(Into::into),
        )
        .await
    }

    async fn apply_text_mutation(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
        expected_sha256: Option<&str>,
        mutation_kind: TextMutationKind,
        apply: impl FnOnce(&str) -> ServiceResult<AppliedText>,
    ) -> ServiceResult<PatchResult> {
        let (node, text) = self.load_text(space_id, node_id).await?;
        let previous_sha256 = text.content_sha256.clone();

        // Check the caller's version before matching either edit format.
        check_expected_sha(expected_sha256, &previous_sha256)?;
        let plain_content = text.content.as_deref().ok_or_else(|| {
            ServiceError::InvalidInput("text content is not stored as plaintext".to_owned())
        })?;
        let applied = apply(plain_content)?;
        let diff = unified_diff(plain_content, &applied.content);

        validate_structured_text(&node.name, &applied.content)?;
        let metrics = content::compute(&applied.content);
        validation::validate_text_content(metrics.byte_len, metrics.line_count)?;

        let stored = metrics.into_stored_plain(applied.content);
        let save_guard = expected_sha256.unwrap_or(&previous_sha256);
        let (node, text) = self
            .store
            .save_text_content(
                space_id,
                node.id,
                &stored,
                Some(save_guard),
                caller_account_id,
                mutation_kind,
            )
            .await?;
        let view = self.text_view(space_id, node, text).await?;

        Ok(PatchResult {
            node: view.node,
            text: view.text,
            previous_sha256,
            edits_applied: applied.replacements,
            diff,
        })
    }

    /// Copy a node within the same space (`cp`). Requires write permission.
    pub async fn copy_node(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        command: CopyNode,
    ) -> ServiceResult<CopyResult> {
        self.authorize(space_id, caller_account_id, FileCommand::Copy)
            .await?;

        let source = self.load_node(space_id, command.node_id).await?;
        if source.kind == NodeKind::Folder && !command.recursive {
            return Err(ServiceError::Conflict(
                "folder copy requires recursive=true".to_owned(),
            ));
        }
        validation::validate_basename(&command.new_name, source.kind)?;

        let (node, counts) = self
            .store
            .copy_node(space_id, &command, caller_account_id)
            .await?;
        let node = self.node_view(space_id, node).await?;
        Ok(CopyResult { node, counts })
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
            .replace_node_metadata(
                space_id,
                node_id,
                &metadata,
                caller_account_id,
                MetadataMutationKind::Replace,
            )
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
            .replace_node_metadata(
                space_id,
                node_id,
                &metadata,
                caller_account_id,
                MetadataMutationKind::Patch,
            )
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

        let dest_parent_path = self.path_of(space_id, command.new_parent_node_id).await?;
        let dest_parent_depth = path_depth(&dest_parent_path);
        let new_path = join_path(&dest_parent_path, &final_name);
        validation::validate_path_len(&new_path)?;
        let subtree_depth = self
            .store
            .subtree_relative_depth(space_id, command.node_id)
            .await?;
        validation::validate_depth(dest_parent_depth + 1 + subtree_depth)?;

        if node.parent_id != Some(command.new_parent_node_id) {
            let children = self
                .store
                .count_live_children(space_id, command.new_parent_node_id)
                .await?;
            let caps = self.effective_limits(space_id).await?;
            validation::validate_fanout(children, caps)?;
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
            .soft_delete_node(space_id, node.id, caller_account_id, command.recursive)
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

fn validate_stored_text_format(name: &str, stored: &StoredContent) -> ServiceResult<()> {
    if let WriteTextBody::Plain(content) = &stored.body {
        validate_structured_text(name, content)?;
    }
    Ok(())
}

fn validate_file_encryption(
    mode: notegate_model::FileEncryptionMode,
    metadata: Option<&Value>,
) -> ServiceResult<()> {
    match mode {
        notegate_model::FileEncryptionMode::None => {
            if metadata.is_some() {
                return Err(ServiceError::InvalidInput(
                    "encryption_metadata must be omitted when encryption_mode=none".to_owned(),
                ));
            }
        }
        notegate_model::FileEncryptionMode::Client => {
            if !metadata.is_some_and(Value::is_object) {
                return Err(ServiceError::InvalidInput(
                    "encryption_metadata must be a JSON object when encryption_mode=client"
                        .to_owned(),
                ));
            }
        }
    }
    Ok(())
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
