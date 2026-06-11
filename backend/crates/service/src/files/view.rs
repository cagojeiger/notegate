use notegate_model::{Node, NodeKind, TextObject};
use uuid::Uuid;

use crate::error::ServiceResult;
use crate::files::{NodeView, TextStats, TextView};

use super::FilesService;

impl FilesService {
    /// Build a [`NodeView`] for an existing node (derives path + has_children).
    pub(super) async fn node_view(&self, space_id: Uuid, node: Node) -> ServiceResult<NodeView> {
        let path = self.path_of(space_id, node.id).await?;
        let has_children = self.store.has_children(space_id, node.id).await?;
        let text = if node.kind == NodeKind::Text {
            self.store.text_stats(space_id, node.id).await?
        } else {
            None
        };
        Ok(NodeView {
            node,
            path,
            has_children,
            text,
        })
    }

    /// Build a [`TextView`] for an existing text node.
    pub(super) async fn text_view(
        &self,
        space_id: Uuid,
        node: Node,
        text: TextObject,
    ) -> ServiceResult<TextView> {
        let node = self.text_node_view(space_id, node, &text).await?;
        Ok(TextView { node, text })
    }

    /// Build a text node view from an already-loaded text, avoiding an
    /// extra metrics lookup through `text_stats`.
    pub(super) async fn text_node_view(
        &self,
        space_id: Uuid,
        node: Node,
        text: &TextObject,
    ) -> ServiceResult<NodeView> {
        let path = self.path_of(space_id, node.id).await?;
        Ok(NodeView {
            node,
            path,
            has_children: false,
            text: Some(stats_from_text(text)),
        })
    }
}

pub(super) fn text_view_at_path(node: Node, path: String, text: TextObject) -> TextView {
    let stats = stats_from_text(&text);
    TextView {
        node: NodeView {
            node,
            path,
            has_children: false,
            text: Some(stats),
        },
        text,
    }
}

fn stats_from_text(text: &TextObject) -> TextStats {
    TextStats {
        content_sha256: text.content_sha256.clone(),
        byte_len: text.byte_len,
        line_count: text.line_count,
    }
}
