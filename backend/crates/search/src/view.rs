//! Search result view hydration over the concrete file repository.

use std::collections::{HashMap, HashSet};

use notegate_db::FilesRepo;
use notegate_model::files::{NodeView, WriteLockSource};
use notegate_model::{Node, NodeKind};
use uuid::Uuid;

use crate::SearchResult;

pub(super) async fn hydrate_node_views(
    store: &FilesRepo,
    space_id: Uuid,
    rows: Vec<(Node, String)>,
) -> SearchResult<Vec<NodeView>> {
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
    let mut write_lock_sources = write_lock_sources_many(store, space_id, &node_ids).await?;

    Ok(rows
        .into_iter()
        .map(|(node, path)| NodeView {
            has_children: has_children.get(&node.id).copied().unwrap_or(false),
            text: text_stats.get(&node.id).cloned(),
            file: file_stats.get(&node.id).cloned(),
            write_lock_sources: write_lock_sources.remove(&node.id).unwrap_or_default(),
            node,
            path,
        })
        .collect())
}

pub(super) async fn write_lock_sources_many(
    store: &FilesRepo,
    space_id: Uuid,
    node_ids: &[Uuid],
) -> SearchResult<HashMap<Uuid, Vec<WriteLockSource>>> {
    let direct_sources = store
        .direct_write_lock_ancestors_many(space_id, node_ids)
        .await?;
    if direct_sources.is_empty() {
        return Ok(HashMap::new());
    }
    let source_ids: Vec<Uuid> = direct_sources
        .values()
        .flat_map(|sources| sources.iter().map(|(node_id, _)| *node_id))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let paths = store.node_paths_many(space_id, &source_ids).await?;

    Ok(direct_sources
        .into_iter()
        .map(|(node_id, sources)| {
            let sources = sources
                .into_iter()
                .filter_map(|(source_id, name)| {
                    paths.get(&source_id).map(|path| WriteLockSource {
                        node_id: source_id,
                        name,
                        path: path.clone(),
                    })
                })
                .collect();
            (node_id, sources)
        })
        .collect())
}
