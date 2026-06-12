//! `tree`: deterministic DFS over node summaries.

use notegate_core::limits;
use notegate_model::NodeKind;

use crate::error::{ServiceError, ServiceResult};
use crate::files::policy::FileCommand;
use crate::pagination::clamp_limit;

use super::{
    SearchService, TreeCursor, TreeFrame, TreePage, TreeRequest, child_cursor, join_path,
    search_fingerprint,
};

impl SearchService {
    /// List a subtree as path-first node summaries. Requires read permission.
    pub async fn tree(
        &self,
        caller_account_id: uuid::Uuid,
        space_id: uuid::Uuid,
        request: TreeRequest,
    ) -> ServiceResult<TreePage> {
        self.authorize(space_id, caller_account_id, FileCommand::Ls)
            .await?;
        let scope_node_id = self
            .resolve_scope_folder(space_id, request.path.as_deref())
            .await?;
        let depth = clamp_tree_depth(request.depth);
        let limit = clamp_limit(
            request.limit,
            limits::CHILDREN_DEFAULT_LIMIT,
            limits::CHILDREN_MAX_LIMIT,
        );
        let fingerprint = search_fingerprint(&[
            space_id.to_string(),
            "tree".to_owned(),
            scope_node_id.to_string(),
            depth.to_string(),
            "dfs-sort_order-name-id".to_owned(),
        ]);
        let mut stack =
            self.decode_tree_cursor(request.cursor.as_deref(), &fingerprint, scope_node_id)?;

        let mut items = Vec::with_capacity(limit as usize);
        let mut scanned = 0usize;
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

                let child_depth = frame.depth + 1;
                let path = join_path(&parent_path, &child.name);
                let is_descendable_folder = child.kind == NodeKind::Folder && child_depth < depth;
                items.push(self.node_view(space_id, child.clone(), path).await?);

                if is_descendable_folder {
                    stack.push(TreeFrame {
                        folder_node_id: child.id,
                        depth: child_depth,
                        after: None,
                    });
                    stopped_early = true;
                    break;
                }

                if items.len() >= limit as usize || scanned >= limits::SEARCH_NODE_SCAN_MAX {
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
            self.encode_tree_cursor(fingerprint, stack)?
        } else {
            None
        };

        Ok(TreePage {
            items,
            depth,
            limit,
            has_more,
            next_cursor,
        })
    }

    fn decode_tree_cursor(
        &self,
        raw: Option<&str>,
        fingerprint: &str,
        scope_node_id: uuid::Uuid,
    ) -> ServiceResult<Vec<TreeFrame>> {
        match raw {
            None => Ok(vec![TreeFrame {
                folder_node_id: scope_node_id,
                depth: 0,
                after: None,
            }]),
            Some(raw) => {
                let cursor: TreeCursor = crate::cursor::decode(raw)?;
                if cursor.version != 1
                    || cursor.command != "tree"
                    || cursor.fingerprint != fingerprint
                {
                    return Err(ServiceError::InvalidInput(
                        "tree cursor does not match this query".to_owned(),
                    ));
                }
                Ok(cursor.stack)
            }
        }
    }

    fn encode_tree_cursor(
        &self,
        fingerprint: String,
        stack: Vec<TreeFrame>,
    ) -> ServiceResult<Option<String>> {
        if stack.is_empty() {
            return Ok(None);
        }
        crate::cursor::encode(&TreeCursor {
            version: 1,
            command: "tree".to_owned(),
            fingerprint,
            stack,
        })
        .map(Some)
        .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))
    }
}

fn clamp_tree_depth(depth: Option<i64>) -> i64 {
    match depth {
        None => 2,
        Some(value) if value < 1 => 1,
        Some(value) => value.min(limits::MAX_PATH_DEPTH as i64),
    }
}
