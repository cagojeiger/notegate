//! `grep`: deterministic DFS over plain text content.

use notegate_core::limits;
use notegate_model::{Node, NodeKind, TextStorageFormat};
use std::collections::HashMap;
use uuid::Uuid;

use crate::error::ServiceResult;
use crate::files::policy::FileCommand;
use crate::pagination::clamp_limit;

use super::{
    ContentMatcher, DfsFrame, GrepPage, GrepRequest, PathFilters, SearchService, child_cursor,
    join_path, search_fingerprint, validate_query,
};

impl SearchService {
    /// Grep text content: return plain text nodes whose content contains `q`.
    pub async fn grep(
        &self,
        caller_account_id: uuid::Uuid,
        space_id: uuid::Uuid,
        request: GrepRequest,
    ) -> ServiceResult<GrepPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Grep)
            .await?;
        let q = validate_query(&request.q)?.to_owned();
        let limit = clamp_limit(
            request.limit,
            limits::GREP_DEFAULT_LIMIT,
            limits::GREP_MAX_LIMIT,
        );
        let scope_node_id = self
            .resolve_scope_folder(space_id, request.path.as_deref())
            .await?;
        let fingerprint = search_fingerprint(&[
            space_id.to_string(),
            "grep".to_owned(),
            q.clone(),
            request.match_mode.as_str().to_owned(),
            request.include.join(","),
            request.exclude.join(","),
            scope_node_id.to_string(),
            "case-insensitive".to_owned(),
            "dfs-sort_order-name-id".to_owned(),
        ]);
        let mut stack = self.decode_search_cursor(
            request.cursor.as_deref(),
            "grep",
            &fingerprint,
            scope_node_id,
        )?;

        let mut items = Vec::with_capacity(limit as usize);
        let mut scanned_nodes = 0usize;
        let mut scanned_text_bytes = 0usize;
        let matcher = ContentMatcher::new(&q, request.match_mode)?;
        let path_filters = PathFilters::new(&request.include, &request.exclude)?;

        while !stack.is_empty()
            && items.len() < limit as usize
            && scanned_nodes < limits::SEARCH_NODE_SCAN_MAX
            && scanned_text_bytes < limits::GREP_SCAN_MAX_BYTES
        {
            let Some(frame) = stack.last().cloned() else {
                break;
            };
            let parent_path = self
                .store
                .node_path(space_id, frame.folder_node_id)
                .await?
                .unwrap_or_else(|| "/".to_owned());
            let (children, has_more_children) = self
                .store
                .paged_children(
                    space_id,
                    frame.folder_node_id,
                    limits::SEARCH_CHILDREN_PAGE_MAX,
                    frame.after.as_ref(),
                )
                .await?;

            if children.is_empty() {
                stack.pop();
                continue;
            }

            let text_batch_size = grep_text_batch_size();
            let mut texts = HashMap::new();
            let mut stopped_early = false;
            for (index, child) in children.iter().enumerate() {
                scanned_nodes += 1;
                let path = join_path(&parent_path, &child.name);
                let is_folder = child.kind == NodeKind::Folder;

                if child.kind == NodeKind::Text && path_filters.allows(&path) {
                    if !texts.contains_key(&child.id) {
                        let ids = text_batch_ids(
                            &children,
                            index,
                            &parent_path,
                            &path_filters,
                            text_batch_size,
                        );
                        texts = self.store.find_texts(space_id, &ids).await?;
                    }
                    let Some(text) = texts.get(&child.id) else {
                        if let Some(top) = stack.last_mut() {
                            top.after = Some(child_cursor(child));
                        }
                        continue;
                    };
                    if text.storage_format == TextStorageFormat::Plain {
                        let byte_len = text.byte_len.max(0) as usize;
                        if scanned_text_bytes + byte_len > limits::GREP_SCAN_MAX_BYTES {
                            stopped_early = true;
                            break;
                        }
                        scanned_text_bytes += byte_len;
                        if text
                            .content
                            .as_deref()
                            .is_some_and(|content| matcher.is_match(content))
                        {
                            items.push(self.text_node_view(child.clone(), path, text));
                        }
                    }
                }

                if let Some(top) = stack.last_mut() {
                    top.after = Some(child_cursor(child));
                }
                if is_folder {
                    stack.push(DfsFrame {
                        folder_node_id: child.id,
                        after: None,
                    });
                    stopped_early = true;
                    break;
                }
                if items.len() >= limit as usize
                    || scanned_nodes >= limits::SEARCH_NODE_SCAN_MAX
                    || scanned_text_bytes >= limits::GREP_SCAN_MAX_BYTES
                {
                    stopped_early = true;
                    break;
                }
            }

            if !stopped_early && !has_more_children {
                let should_pop = stack
                    .last()
                    .is_some_and(|top| top.folder_node_id == frame.folder_node_id);
                if should_pop {
                    stack.pop();
                }
            }
        }

        let has_more = !stack.is_empty();
        let next_cursor = if has_more {
            self.encode_search_cursor("grep", fingerprint, stack)?
        } else {
            None
        };

        Ok(GrepPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }
}

fn grep_text_batch_size() -> usize {
    (limits::GREP_SCAN_MAX_BYTES / limits::TEXT_MAX_BYTES).max(1)
}

fn text_batch_ids(
    children: &[Node],
    start_index: usize,
    parent_path: &str,
    path_filters: &PathFilters,
    max_ids: usize,
) -> Vec<Uuid> {
    let mut ids = Vec::new();
    for child in children.iter().skip(start_index) {
        if child.kind == NodeKind::Folder {
            break;
        }
        if child.kind == NodeKind::Text {
            let path = join_path(parent_path, &child.name);
            if path_filters.allows(&path) {
                ids.push(child.id);
                if ids.len() >= max_ids {
                    break;
                }
            }
        }
    }
    ids
}
