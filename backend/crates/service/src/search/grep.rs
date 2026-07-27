//! `grep`: deterministic DFS over plain text content.

use std::collections::HashMap;
use std::sync::Arc;

use notegate_core::limits;
use notegate_model::search::SearchTextCandidate;

use crate::error::ServiceResult;
use crate::files::policy::FileCommand;
use crate::pagination::clamp_limit;

use super::{
    ContentMatcher, GrepPage, GrepRequest, PathFilters, SearchService, decode_search_cursor,
    encode_search_cursor, search_fingerprint, validate_query,
};

#[derive(Debug, PartialEq, Eq)]
struct GrepCandidatePlan {
    planned_candidates: usize,
    body_candidates: Vec<(uuid::Uuid, String, i64)>,
}

fn plan_grep_candidates(
    candidates: &[SearchTextCandidate],
    path_filters: &PathFilters,
    scan_limit: usize,
    byte_budget: usize,
) -> GrepCandidatePlan {
    let mut planned_candidates = 0usize;
    let mut scanned_text_bytes = 0usize;
    let mut body_candidates = Vec::new();
    for candidate in candidates.iter().take(scan_limit) {
        if !path_filters.allows(&candidate.path) {
            planned_candidates += 1;
            continue;
        }

        let byte_len = candidate.byte_len.max(0) as usize;
        if scanned_text_bytes + byte_len > byte_budget {
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
    GrepCandidatePlan {
        planned_candidates,
        body_candidates,
    }
}

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
        let after_sort_path = decode_search_cursor(
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

        let GrepCandidatePlan {
            planned_candidates,
            body_candidates,
        } = plan_grep_candidates(
            &candidates,
            &path_filters,
            limits::SEARCH_NODE_SCAN_MAX,
            limits::GREP_SCAN_MAX_BYTES,
        );
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
            encode_search_cursor("grep", fingerprint, scope_node_id, after)?
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

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::unwrap_used)]

    use chrono::Utc;
    use notegate_model::{Node, NodeKind, TextAtRestEncryption};

    use super::*;

    fn candidate(path: &str, byte_len: i64) -> SearchTextCandidate {
        let now = Utc::now();
        let space_id = uuid::Uuid::new_v4();
        SearchTextCandidate {
            node: Node {
                id: uuid::Uuid::new_v4(),
                space_id,
                parent_id: Some(uuid::Uuid::new_v4()),
                name: path.rsplit('/').next().unwrap().to_owned(),
                kind: NodeKind::Text,
                sort_order: 0,
                metadata: serde_json::Value::Null,
                search_enabled: true,
                write_locked: false,
                created_by_account_id: uuid::Uuid::new_v4(),
                updated_by_account_id: uuid::Uuid::new_v4(),
                deleted_by_account_id: None,
                purge_after: None,
                created_at: now,
                updated_at: now,
                deleted_at: None,
            },
            path: path.to_owned(),
            sort_path: path.to_owned(),
            content_sha256: format!("sha:{path}"),
            byte_len,
            line_count: 1,
            at_rest_encryption: TextAtRestEncryption::None,
        }
    }

    #[test]
    fn candidate_plan_consumes_filtered_rows_without_spending_byte_budget() {
        let candidates = vec![
            candidate("/private/hidden.md", 100),
            candidate("/notes/first.md", 4),
            candidate("/notes/over-budget.md", 7),
            candidate("/notes/not-reached.md", 1),
        ];
        let filters = PathFilters::new(&[], &["/private/*".to_owned()]).unwrap();

        let plan = plan_grep_candidates(&candidates, &filters, usize::MAX, 10);

        assert_eq!(plan.planned_candidates, 2);
        assert_eq!(
            plan.body_candidates,
            vec![(
                candidates[1].node.id,
                candidates[1].content_sha256.clone(),
                4,
            )]
        );
    }

    #[test]
    fn candidate_plan_accepts_exact_budget_and_obeys_scan_limit() {
        let candidates = vec![
            candidate("/notes/zero.md", -1),
            candidate("/notes/exact.md", 5),
            candidate("/notes/outside-scan.md", 0),
        ];
        let filters = PathFilters::new(&[], &[]).unwrap();

        let plan = plan_grep_candidates(&candidates, &filters, 2, 5);

        assert_eq!(plan.planned_candidates, 2);
        assert_eq!(
            plan.body_candidates,
            vec![
                (
                    candidates[0].node.id,
                    candidates[0].content_sha256.clone(),
                    -1,
                ),
                (
                    candidates[1].node.id,
                    candidates[1].content_sha256.clone(),
                    5,
                ),
            ]
        );
    }
}
