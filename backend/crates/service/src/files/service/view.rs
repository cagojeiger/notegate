use notegate_model::{Document, Node, NodeKind};
use uuid::Uuid;

use crate::error::ServiceResult;
use crate::files::{DocumentStats, DocumentView, FilesStore, NodeView};

use super::FilesService;

impl<S> FilesService<S>
where
    S: FilesStore,
{
    /// Build a [`NodeView`] for an existing node (derives path + has_children).
    pub(super) async fn node_view(
        &self,
        workspace_id: Uuid,
        node: Node,
    ) -> ServiceResult<NodeView> {
        let path = self.path_of(workspace_id, node.id).await?;
        let has_children = self.store.has_children(workspace_id, node.id).await?;
        let document = if node.kind == NodeKind::Document {
            self.store.document_stats(workspace_id, node.id).await?
        } else {
            None
        };
        Ok(NodeView {
            node,
            path,
            has_children,
            document,
        })
    }

    /// Build a [`DocumentView`] for an existing document node.
    pub(super) async fn document_view(
        &self,
        workspace_id: Uuid,
        node: Node,
        document: Document,
    ) -> ServiceResult<DocumentView> {
        let node = self
            .document_node_view(workspace_id, node, &document)
            .await?;
        Ok(DocumentView { node, document })
    }

    /// Build a document node view from an already-loaded document, avoiding an
    /// extra metrics lookup through [`FilesStore::document_stats`].
    pub(super) async fn document_node_view(
        &self,
        workspace_id: Uuid,
        node: Node,
        document: &Document,
    ) -> ServiceResult<NodeView> {
        let path = self.path_of(workspace_id, node.id).await?;
        Ok(NodeView {
            node,
            path,
            has_children: false,
            document: Some(stats_from_document(document)),
        })
    }
}

pub(super) fn document_view_at_path(node: Node, path: String, document: Document) -> DocumentView {
    let stats = stats_from_document(&document);
    DocumentView {
        node: NodeView {
            node,
            path,
            has_children: false,
            document: Some(stats),
        },
        document,
    }
}

fn stats_from_document(document: &Document) -> DocumentStats {
    DocumentStats {
        content_sha256: document.content_sha256.clone(),
        byte_len: document.byte_len,
        line_count: document.line_count,
    }
}
