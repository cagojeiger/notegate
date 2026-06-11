//! `find`: deterministic DFS over node names.

use notegate_core::limits;

use crate::error::ServiceResult;
use crate::files::policy::FileCommand;
use crate::pagination::clamp_limit;

use super::{
    FindPage, FindRequest, NameMatcher, SearchService, search_fingerprint, validate_query,
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
        let scope_path = self
            .store
            .node_path(space_id, scope_node_id)
            .await?
            .unwrap_or_else(|| "/".to_owned());
        let fingerprint = search_fingerprint(&[
            space_id.to_string(),
            "find".to_owned(),
            q.clone(),
            request
                .kind
                .map(|kind| kind.as_str().to_owned())
                .unwrap_or_default(),
            request.match_mode.as_str().to_owned(),
            scope_node_id.to_string(),
            "case-insensitive".to_owned(),
            "dfs-sort_order-name-id".to_owned(),
        ]);
        let after_sort_path = self.decode_search_cursor(
            request.cursor.as_deref(),
            "find",
            &fingerprint,
            scope_node_id,
        )?;

        let matcher = NameMatcher::new(&q, request.match_mode)?;
        let candidates = self
            .store
            .search_node_candidates(
                space_id,
                scope_node_id,
                &scope_path,
                after_sort_path.as_deref(),
                limits::SEARCH_CANDIDATE_PAGE_MAX + 1,
            )
            .await?;

        let mut items = Vec::with_capacity(limit as usize);
        let mut consumed = 0usize;
        let mut after = None;
        for candidate in candidates.iter().take(limits::SEARCH_NODE_SCAN_MAX) {
            consumed += 1;
            after = Some(candidate.sort_path.clone());
            let kind_matches = request.kind.is_none_or(|kind| kind == candidate.node.kind);
            let name_matches = matcher.is_match(&candidate.node.name);
            if kind_matches && name_matches {
                items.push(
                    self.node_view(space_id, candidate.node.clone(), candidate.path.clone())
                        .await?,
                );
            }
            if items.len() >= limit as usize {
                break;
            }
        }

        let has_more = candidates.len() > consumed;
        let next_cursor = if has_more {
            self.encode_search_cursor("find", fingerprint, scope_node_id, after)?
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
