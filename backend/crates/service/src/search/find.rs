//! `find`: deterministic DFS over node names.

use notegate_core::limits;
use notegate_model::NodeKind;

use crate::error::ServiceResult;
use crate::files::policy::FileCommand;
use crate::pagination::clamp_limit;

use super::{
    DfsFrame, FindPage, FindRequest, NameMatcher, SearchService, child_cursor, join_path,
    search_fingerprint, validate_query,
};

impl SearchService {
    /// Find nodes by name, optionally filtered by `kind` and scoped to a folder subtree.
    pub async fn find(
        &self,
        caller_account_id: uuid::Uuid,
        space_id: uuid::Uuid,
        request: FindRequest,
    ) -> ServiceResult<FindPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Find)
            .await?;
        let q = validate_query(&request.q)?.to_owned();
        let limit = clamp_limit(
            request.limit,
            limits::FIND_DEFAULT_LIMIT,
            limits::FIND_MAX_LIMIT,
        );
        let scope_node_id = self
            .resolve_scope_folder(space_id, request.path.as_deref())
            .await?;
        let fingerprint = search_fingerprint(&[
            "find".to_owned(),
            q.clone(),
            request
                .kind
                .map(|kind| kind.as_str().to_owned())
                .unwrap_or_default(),
            request.match_mode.as_str().to_owned(),
            scope_node_id.to_string(),
            "dfs-sort_order-name-id".to_owned(),
        ]);
        let mut stack = self.decode_search_cursor(
            request.cursor.as_deref(),
            "find",
            &fingerprint,
            scope_node_id,
        )?;

        let mut items = Vec::with_capacity(limit as usize);
        let mut scanned = 0usize;
        let matcher = NameMatcher::new(&q, request.match_mode)?;

        while !stack.is_empty()
            && items.len() < limit as usize
            && scanned < limits::SEARCH_NODE_SCAN_MAX
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

            let mut stopped_early = false;
            for child in children {
                scanned += 1;
                if let Some(top) = stack.last_mut() {
                    top.after = Some(child_cursor(&child));
                }
                let path = join_path(&parent_path, &child.name);
                let is_folder = child.kind == NodeKind::Folder;
                if is_folder {
                    stack.push(DfsFrame {
                        folder_node_id: child.id,
                        after: None,
                    });
                }

                let kind_matches = request.kind.is_none_or(|kind| kind == child.kind);
                let name_matches = matcher.is_match(&child.name);
                if kind_matches && name_matches {
                    items.push(self.node_view(space_id, child, path).await?);
                    if items.len() >= limit as usize {
                        stopped_early = true;
                        break;
                    }
                }
                if scanned >= limits::SEARCH_NODE_SCAN_MAX {
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
            self.encode_search_cursor("find", fingerprint, stack)?
        } else {
            None
        };

        Ok(FindPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }
}
