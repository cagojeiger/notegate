//! `find`: deterministic DFS over node names.

use notegate_core::limits;
use notegate_model::search::SearchNodeCandidate;
use notegate_model::{Node, NodeKind};

use crate::error::ServiceResult;
use crate::files::policy::FileCommand;
use crate::pagination::clamp_limit;

use super::telemetry::{SearchOperation, SearchStage};
use super::{
    FindPage, FindRequest, NameMatcher, PathFilters, SearchService, decode_search_cursor,
    encode_search_cursor, search_fingerprint, validate_query,
};

#[derive(Debug, PartialEq, Eq)]
struct FindCandidateReduction {
    items: Vec<(Node, String)>,
    after_sort_path: Option<String>,
    has_more: bool,
}

fn reduce_find_candidates(
    candidates: &[SearchNodeCandidate],
    kind: Option<NodeKind>,
    matcher: &NameMatcher,
    path_filters: &PathFilters,
    result_limit: usize,
    scan_limit: usize,
) -> FindCandidateReduction {
    let mut items = Vec::with_capacity(result_limit);
    let mut consumed = 0usize;
    let mut after_sort_path = None;
    for candidate in candidates.iter().take(scan_limit) {
        consumed += 1;
        after_sort_path = Some(candidate.sort_path.clone());
        let kind_matches = kind.is_none_or(|kind| kind == candidate.node.kind);
        let path_matches = path_filters.allows(&candidate.path);
        let name_matches = matcher.is_match(&candidate.node.name);
        if kind_matches && path_matches && name_matches {
            items.push((candidate.node.clone(), candidate.path.clone()));
        }
        if items.len() >= result_limit {
            break;
        }
    }

    FindCandidateReduction {
        items,
        after_sort_path,
        has_more: candidates.len() > consumed,
    }
}

impl SearchService {
    /// Find nodes by name, optionally filtered by `kind` and scoped to a folder subtree.
    pub async fn find(
        &self,
        caller_account_id: uuid::Uuid,
        space_id: uuid::Uuid,
        request: FindRequest,
    ) -> ServiceResult<FindPage> {
        let operation = SearchOperation::Find;
        let timer = self
            .telemetry
            .operation(operation, request.match_mode.as_str());
        let result = async {
            self.telemetry
                .stage(
                    operation,
                    SearchStage::Authorize,
                    self.authorize(space_id, caller_account_id, FileCommand::Find),
                )
                .await?;
            let q = validate_query(&request.q)?.to_owned();
            let limit = clamp_limit(
                request.limit,
                limits::FIND_DEFAULT_LIMIT,
                limits::FIND_MAX_LIMIT,
            );
            let (scope_node_id, scope_path) = self
                .telemetry
                .stage(
                    operation,
                    SearchStage::ResolveScope,
                    self.resolve_scope_folder(space_id, request.path.as_deref()),
                )
                .await?;
            let (fingerprint, after_sort_path, matcher, path_filters) =
                self.telemetry
                    .stage_sync(operation, SearchStage::Prepare, || {
                        let fingerprint = search_fingerprint(&[
                            space_id.to_string(),
                            "find".to_owned(),
                            q.clone(),
                            request
                                .kind
                                .map(|kind| kind.as_str().to_owned())
                                .unwrap_or_default(),
                            request.match_mode.as_str().to_owned(),
                            request.include.join(","),
                            request.exclude.join(","),
                            scope_node_id.to_string(),
                            "case-insensitive".to_owned(),
                            "dfs-sort_order-name-id".to_owned(),
                        ]);
                        let after_sort_path = decode_search_cursor(
                            request.cursor.as_deref(),
                            "find",
                            &fingerprint,
                            scope_node_id,
                        )?;
                        let matcher = NameMatcher::new(&q, request.match_mode)?;
                        let path_filters = PathFilters::new(&request.include, &request.exclude)?;
                        Ok::<_, crate::error::ServiceError>((
                            fingerprint,
                            after_sort_path,
                            matcher,
                            path_filters,
                        ))
                    })?;
            let candidates = self
                .telemetry
                .stage(
                    operation,
                    SearchStage::CandidateQuery,
                    self.store.search_node_candidates(
                        space_id,
                        scope_node_id,
                        &scope_path,
                        after_sort_path.as_deref(),
                        limits::SEARCH_CANDIDATE_PAGE_MAX + 1,
                    ),
                )
                .await?;

            let FindCandidateReduction {
                items,
                after_sort_path,
                has_more,
            } = self
                .telemetry
                .stage_sync(operation, SearchStage::MatchReduce, || {
                    reduce_find_candidates(
                        &candidates,
                        request.kind,
                        &matcher,
                        &path_filters,
                        limit as usize,
                        limits::SEARCH_NODE_SCAN_MAX,
                    )
                });
            let next_cursor = if has_more {
                encode_search_cursor("find", fingerprint, scope_node_id, after_sort_path)?
            } else {
                None
            };
            let result_count = items.len();
            let items = self
                .telemetry
                .stage(
                    operation,
                    SearchStage::Hydrate,
                    self.node_views(space_id, items),
                )
                .await?;
            self.telemetry
                .record_workload(operation, candidates.len(), result_count, 0, 0);

            Ok(FindPage {
                items,
                limit,
                has_more,
                next_cursor,
            })
        }
        .await;
        timer.finish(&result);
        result
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::unwrap_used)]

    use chrono::Utc;

    use super::super::FindMatchMode;
    use super::*;

    fn candidate(path: &str, kind: NodeKind) -> SearchNodeCandidate {
        let now = Utc::now();
        SearchNodeCandidate {
            node: Node {
                id: uuid::Uuid::new_v4(),
                space_id: uuid::Uuid::new_v4(),
                parent_id: Some(uuid::Uuid::new_v4()),
                name: path.rsplit('/').next().unwrap().to_owned(),
                kind,
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
        }
    }

    #[test]
    fn reduction_filters_candidates_and_stops_at_result_limit() {
        let candidates = vec![
            candidate("/private/note-0.md", NodeKind::Text),
            candidate("/notes/readme.md", NodeKind::Text),
            candidate("/notes/note-folder", NodeKind::Folder),
            candidate("/notes/note-1.md", NodeKind::Text),
            candidate("/notes/note-2.md", NodeKind::Text),
            candidate("/notes/note-3.md", NodeKind::Text),
        ];
        let matcher = NameMatcher::new("note", FindMatchMode::Contains).unwrap();
        let filters = PathFilters::new(&[], &["/private/*".to_owned()]).unwrap();

        let reduction = reduce_find_candidates(
            &candidates,
            Some(NodeKind::Text),
            &matcher,
            &filters,
            2,
            usize::MAX,
        );

        assert_eq!(
            reduction
                .items
                .iter()
                .map(|(node, _)| node.id)
                .collect::<Vec<_>>(),
            vec![candidates[3].node.id, candidates[4].node.id]
        );
        assert_eq!(
            reduction.after_sort_path,
            Some(candidates[4].sort_path.clone())
        );
        assert!(reduction.has_more);
    }

    #[test]
    fn reduction_advances_through_a_non_matching_scan_window() {
        let candidates = vec![
            candidate("/notes/alpha.md", NodeKind::Text),
            candidate("/notes/beta.md", NodeKind::Text),
            candidate("/notes/note.md", NodeKind::Text),
        ];
        let matcher = NameMatcher::new("note", FindMatchMode::Contains).unwrap();
        let filters = PathFilters::new(&[], &[]).unwrap();

        let reduction = reduce_find_candidates(&candidates, None, &matcher, &filters, 10, 2);

        assert!(reduction.items.is_empty());
        assert_eq!(
            reduction.after_sort_path,
            Some(candidates[1].sort_path.clone())
        );
        assert!(reduction.has_more);
    }

    #[test]
    fn reduction_does_not_report_more_when_final_candidate_fills_page() {
        let candidates = vec![
            candidate("/notes/note-1.md", NodeKind::Text),
            candidate("/notes/note-2.md", NodeKind::Text),
        ];
        let matcher = NameMatcher::new("note", FindMatchMode::Contains).unwrap();
        let filters = PathFilters::new(&[], &[]).unwrap();

        let reduction =
            reduce_find_candidates(&candidates, None, &matcher, &filters, 2, usize::MAX);

        assert_eq!(reduction.items.len(), 2);
        assert_eq!(
            reduction.after_sort_path,
            Some(candidates[1].sort_path.clone())
        );
        assert!(!reduction.has_more);
    }
}
