//! `grep`: deterministic DFS over plain text content.

use std::collections::HashMap;
use std::sync::Arc;

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
        let (scope_node_id, scope_path) = self
            .resolve_scope_folder(space_id, request.path.as_deref())
            .await?;
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

        let mut planned_candidates = 0usize;
        let mut scanned_text_bytes = 0usize;
        let mut body_candidates = Vec::new();
        for candidate in candidates.iter().take(limits::SEARCH_NODE_SCAN_MAX) {
            if !path_filters.allows(&candidate.path) {
                planned_candidates += 1;
                continue;
            }

            let byte_len = candidate.byte_len.max(0) as usize;
            if scanned_text_bytes + byte_len > limits::GREP_SCAN_MAX_BYTES {
                break;
            }
            scanned_text_bytes += byte_len;
            planned_candidates += 1;
            body_candidates.push((
                candidate.node.id,
                candidate.content_sha256.clone(),
                candidate.byte_len,
            ));
        }
        let mut bodies = HashMap::with_capacity(body_candidates.len());
        let mut missing_body_candidates = Vec::new();
        for (node_id, content_sha256, byte_len) in &body_candidates {
            // A hit is consistent with the candidate metadata scan. A write
            // racing after that scan is observed through its new SHA by the
            // next request.
            if let Some(body) = self
                .body_cache
                .get(space_id, *node_id, content_sha256)
                .await
            {
                bodies.insert(*node_id, body);
            } else {
                missing_body_candidates.push((*node_id, content_sha256.clone(), *byte_len));
            }
        }

        if !missing_body_candidates.is_empty() {
            let _load_guard = self
                .body_cache
                .lock_misses(space_id, &missing_body_candidates)
                .await;
            let mut candidates_to_load = Vec::new();
            for (node_id, content_sha256, byte_len) in missing_body_candidates {
                if let Some(body) = self
                    .body_cache
                    .get(space_id, node_id, &content_sha256)
                    .await
                {
                    bodies.insert(node_id, body);
                } else {
                    candidates_to_load.push((node_id, content_sha256, byte_len));
                }
            }

            let loaded_bodies = self
                .store
                .search_text_bodies_within_budget(
                    space_id,
                    &candidates_to_load,
                    limits::GREP_SCAN_MAX_BYTES,
                )
                .await?;
            for (node_id, text) in loaded_bodies {
                let content_sha256 = text.content_sha256;
                let Some(content) = text.content else {
                    continue;
                };
                let body = Arc::<str>::from(content);
                self.body_cache
                    .insert(space_id, node_id, &content_sha256, Arc::clone(&body))
                    .await;
                bodies.insert(node_id, body);
            }
        }

        let mut items = Vec::with_capacity(limit as usize);
        let mut consumed = 0usize;
        let mut after = None;
        for candidate in candidates.iter().take(planned_candidates) {
            if !path_filters.allows(&candidate.path) {
                consumed += 1;
                after = Some(candidate.sort_path.clone());
                continue;
            }

            let Some(content) = bodies.remove(&candidate.node.id) else {
                // The text changed or became ineligible after the candidate
                // scan. Consume the stale candidate so pagination can progress.
                consumed += 1;
                after = Some(candidate.sort_path.clone());
                continue;
            };
            consumed += 1;
            after = Some(candidate.sort_path.clone());

            let match_lines = matcher.match_lines(&content, request.line_mode);
            if !match_lines.is_empty() {
                items.push(notegate_model::search::GrepHit {
                    node: self.text_node_view(candidate),
                    match_lines: match request.line_mode {
                        notegate_model::search::GrepLineMode::None => Vec::new(),
                        notegate_model::search::GrepLineMode::First => {
                            match_lines.first().copied().into_iter().collect()
                        }
                        notegate_model::search::GrepLineMode::All => match_lines,
                    },
                });
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
        if !items.is_empty() {
            let node_ids: Vec<_> = items.iter().map(|item| item.node.node.id).collect();
            let mut write_lock_sources =
                crate::files::write_lock_sources_many(&self.store, space_id, &node_ids).await?;
            for item in &mut items {
                item.node.write_lock_sources = write_lock_sources
                    .remove(&item.node.node.id)
                    .unwrap_or_default();
            }
        }

        Ok(GrepPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }
}
