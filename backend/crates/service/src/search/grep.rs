//! `grep`: deterministic DFS over plain text content.

use notegate_core::limits;

use crate::error::ServiceResult;
use crate::files::policy::FileCommand;
use crate::pagination::clamp_limit;

use super::{
    ContentMatcher, GrepPage, GrepRequest, PathFilters, SearchService, search_fingerprint,
    validate_query,
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
        let scope_path = self
            .store
            .node_path(space_id, scope_node_id)
            .await?
            .unwrap_or_else(|| "/".to_owned());
        let fingerprint = search_fingerprint(&[
            space_id.to_string(),
            "grep".to_owned(),
            q.clone(),
            request.match_mode.as_str().to_owned(),
            request.line_mode.as_str().to_owned(),
            request.include.join(","),
            request.exclude.join(","),
            scope_node_id.to_string(),
            "case-insensitive".to_owned(),
            "dfs-sort_order-name-id".to_owned(),
        ]);
        let after_sort_path = self.decode_search_cursor(
            request.cursor.as_deref(),
            "grep",
            &fingerprint,
            scope_node_id,
        )?;

        let matcher = ContentMatcher::new(&q, request.match_mode)?;
        let path_filters = PathFilters::new(&request.include, &request.exclude)?;
        let candidates = self
            .store
            .search_text_candidates(
                space_id,
                scope_node_id,
                &scope_path,
                after_sort_path.as_deref(),
                limits::SEARCH_CANDIDATE_PAGE_MAX + 1,
            )
            .await?;

        let mut items = Vec::with_capacity(limit as usize);
        let mut consumed = 0usize;
        let mut scanned_text_bytes = 0usize;
        let mut after = None;
        for candidate in candidates.iter().take(limits::SEARCH_NODE_SCAN_MAX) {
            if !path_filters.allows(&candidate.path) {
                consumed += 1;
                after = Some(candidate.sort_path.clone());
                continue;
            }

            let byte_len = candidate.text.byte_len.max(0) as usize;
            if scanned_text_bytes + byte_len > limits::GREP_SCAN_MAX_BYTES {
                break;
            }
            scanned_text_bytes += byte_len;
            consumed += 1;
            after = Some(candidate.sort_path.clone());

            if let Some(content) = candidate.text.content.as_deref() {
                let match_lines = matcher.match_lines(content, request.line_mode);
                if !match_lines.is_empty() {
                    items.push(notegate_model::search::GrepHit {
                        node: self.text_node_view(
                            candidate.node.clone(),
                            candidate.path.clone(),
                            &candidate.text,
                        ),
                        match_lines: match request.line_mode {
                            notegate_model::search::GrepLineMode::None => Vec::new(),
                            notegate_model::search::GrepLineMode::First => {
                                match_lines.first().copied().into_iter().collect()
                            }
                            notegate_model::search::GrepLineMode::All => match_lines,
                        },
                    });
                }
            }

            if items.len() >= limit as usize {
                break;
            }
        }

        let has_more = candidates.len() > consumed;
        let next_cursor = if has_more {
            self.encode_search_cursor("grep", fingerprint, scope_node_id, after)?
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
