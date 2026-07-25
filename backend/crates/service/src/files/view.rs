use notegate_model::{FileObject, Node, NodeKind, NodeSummary, TextObject};
use uuid::Uuid;

use crate::error::ServiceResult;
use crate::files::{FileStats, FileView, NodeSummaryView, NodeView, TextStats, TextView};
use notegate_db::FilesRepo;

use super::FilesService;

pub(crate) async fn hydrate_node_views(
    store: &FilesRepo,
    space_id: Uuid,
    rows: Vec<(Node, String)>,
) -> ServiceResult<Vec<NodeView>> {
    let node_ids: Vec<Uuid> = rows.iter().map(|(node, _)| node.id).collect();
    let text_ids: Vec<Uuid> = rows
        .iter()
        .filter(|(node, _)| node.kind == NodeKind::Text)
        .map(|(node, _)| node.id)
        .collect();
    let file_ids: Vec<Uuid> = rows
        .iter()
        .filter(|(node, _)| node.kind == NodeKind::File)
        .map(|(node, _)| node.id)
        .collect();
    let has_children = store.has_children_many(space_id, &node_ids).await?;
    let text_stats = store.text_stats_many(space_id, &text_ids).await?;
    let file_stats = store.file_stats_many(space_id, &file_ids).await?;

    Ok(rows
        .into_iter()
        .map(|(node, path)| NodeView {
            has_children: has_children.get(&node.id).copied().unwrap_or(false),
            text: text_stats.get(&node.id).cloned(),
            file: file_stats.get(&node.id).cloned(),
            node,
            path,
        })
        .collect())
}

pub(crate) async fn hydrate_node_summary_views(
    store: &FilesRepo,
    space_id: Uuid,
    rows: Vec<(NodeSummary, String)>,
) -> ServiceResult<Vec<NodeSummaryView>> {
    let node_ids: Vec<Uuid> = rows.iter().map(|(node, _)| node.id).collect();
    let text_ids: Vec<Uuid> = rows
        .iter()
        .filter(|(node, _)| node.kind == NodeKind::Text)
        .map(|(node, _)| node.id)
        .collect();
    let file_ids: Vec<Uuid> = rows
        .iter()
        .filter(|(node, _)| node.kind == NodeKind::File)
        .map(|(node, _)| node.id)
        .collect();
    let has_children = store.has_children_many(space_id, &node_ids).await?;
    let text_stats = store.text_stats_many(space_id, &text_ids).await?;
    let file_stats = store.file_stats_many(space_id, &file_ids).await?;

    Ok(rows
        .into_iter()
        .map(|(node, path)| NodeSummaryView {
            has_children: has_children.get(&node.id).copied().unwrap_or(false),
            text: text_stats.get(&node.id).cloned(),
            file: file_stats.get(&node.id).cloned(),
            node,
            path,
        })
        .collect())
}

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
        let file = if node.kind == NodeKind::File {
            self.store.file_stats(space_id, node.id).await?
        } else {
            None
        };
        Ok(NodeView {
            node,
            path,
            has_children,
            text,
            file,
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
            file: None,
        })
    }

    /// Build a file node view from an already-loaded file.
    pub(super) async fn file_node_view(
        &self,
        space_id: Uuid,
        node: Node,
        file: &FileObject,
    ) -> ServiceResult<NodeView> {
        let path = self.path_of(space_id, node.id).await?;
        Ok(NodeView {
            node,
            path,
            has_children: false,
            text: None,
            file: Some(stats_from_file(file)),
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
            file: None,
        },
        text,
    }
}

pub(super) fn file_view_at_path(node: Node, path: String, file: FileObject) -> FileView {
    let stats = stats_from_file(&file);
    FileView {
        node: NodeView {
            node,
            path,
            has_children: false,
            text: None,
            file: Some(stats),
        },
        file,
    }
}

fn stats_from_text(text: &TextObject) -> TextStats {
    TextStats {
        content_sha256: text.content_sha256.clone(),
        byte_len: text.byte_len,
        line_count: text.line_count,
    }
}

fn stats_from_file(file: &FileObject) -> FileStats {
    FileStats {
        media_type: file.media_type.clone(),
        detected_media_type: file.detected_media_type.clone(),
        byte_len: file.byte_len,
        original_filename: file.original_filename.clone(),
        encryption_mode: file.encryption_mode,
        encryption_metadata: file.encryption_metadata.clone(),
    }
}
