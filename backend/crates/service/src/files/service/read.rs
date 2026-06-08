use notegate_core::limits;
use notegate_model::NodeKind;
use uuid::Uuid;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::files::validation;
use crate::files::{
    ChildrenCursor, ChildrenPage, ChildrenRequest, FileCommand, NodeView, ReadDocument, ReadResult,
};

use super::range::slice_document;
use super::{FilesService, join_path};
use crate::files::FilesStore;

impl<S> FilesService<S>
where
    S: FilesStore,
{
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
            document: None,
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
        let path = validation::normalize_path(path)?;
        let node_id = self
            .store
            .resolve_path(workspace_id, &path)
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
        let cursor = match request.cursor.as_deref() {
            None => None,
            Some(raw) => Some(cursor::decode::<ChildrenCursor>(raw)?),
        };
        let (rows, has_more) = self
            .store
            .paged_children(workspace_id, parent_node_id, limit, cursor.as_ref())
            .await?;

        let next_cursor = if has_more {
            rows.last()
                .map(|node| ChildrenCursor {
                    sort_order: node.sort_order,
                    name: node.name.clone(),
                    id: node.id,
                })
                .map(|cursor| cursor::encode(&cursor))
                .transpose()
                .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
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
                document: None,
            });
        }

        Ok(ChildrenPage {
            parent: NodeView {
                node: parent,
                path: parent_path,
                has_children: parent_has_children,
                document: None,
            },
            items,
            limit,
            has_more,
            next_cursor,
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
        let view = self
            .document_node_view(workspace_id, node, &document)
            .await?;

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
