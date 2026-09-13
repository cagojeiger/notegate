//! Concrete Postgres-backed search store adapter.

use std::collections::{HashMap, HashSet};

use notegate_db::FilesRepo;
use notegate_model::files::{FileStats, TextStats};
use notegate_model::search::{SearchNodeCandidate, SearchTextCandidate};
use notegate_model::{NodeKind, Permission, TextObject};
use uuid::Uuid;

use crate::{SearchError, SearchResult};

#[derive(Debug, Clone)]
pub(crate) struct PostgresSearchStore {
    // Permission revocation must not wait for replica replay.
    authority_repo: FilesRepo,
    // Candidate, body, and content-stat reads may use a replica.
    query_repo: FilesRepo,
}

impl PostgresSearchStore {
    pub(crate) fn with_authority_and_query_repos(
        authority_repo: FilesRepo,
        query_repo: FilesRepo,
    ) -> Self {
        Self {
            authority_repo: authority_repo.with_external_access_only(true),
            query_repo,
        }
    }

    pub(crate) async fn permission_for(
        &self,
        space_id: Uuid,
        account_id: Uuid,
    ) -> SearchResult<Option<Permission>> {
        Ok(self
            .authority_repo
            .permission_for(space_id, account_id)
            .await?)
    }

    pub(crate) async fn resolve_search_scope(
        &self,
        space_id: Uuid,
        path: &str,
    ) -> SearchResult<Option<(Uuid, NodeKind, String)>> {
        let scope = self
            .authority_repo
            .resolve_search_scope(space_id, path)
            .await?;
        if let Some((node_id, _, _)) = &scope
            && !self
                .accessible_live_node_ids(space_id, &[*node_id])
                .await?
                .contains(node_id)
        {
            return Err(SearchError::NotFound("scope path not found".to_owned()));
        }
        Ok(scope)
    }

    pub(crate) async fn accessible_live_node_ids(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> SearchResult<HashSet<Uuid>> {
        // Replica candidates and cached bodies are not authorization evidence.
        Ok(self
            .authority_repo
            .externally_accessible_node_ids(space_id, node_ids, false)
            .await?)
    }

    pub(crate) async fn search_node_candidates(
        &self,
        space_id: Uuid,
        scope_node_id: Uuid,
        scope_path: &str,
        after_sort_path: Option<&str>,
        limit: i64,
    ) -> SearchResult<Vec<SearchNodeCandidate>> {
        Ok(self
            .authority_repo
            .search_node_candidates(space_id, scope_node_id, scope_path, after_sort_path, limit)
            .await?)
    }

    pub(crate) async fn search_text_candidates(
        &self,
        space_id: Uuid,
        scope_node_id: Uuid,
        scope_path: &str,
        after_sort_path: Option<&str>,
        limit: i64,
    ) -> SearchResult<Vec<SearchTextCandidate>> {
        Ok(self
            .authority_repo
            .search_text_candidates(space_id, scope_node_id, scope_path, after_sort_path, limit)
            .await?)
    }

    pub(crate) async fn search_text_bodies_within_budget(
        &self,
        space_id: Uuid,
        candidates: &[(Uuid, String, i64)],
        max_bytes: usize,
    ) -> SearchResult<HashMap<Uuid, TextObject>> {
        Ok(self
            .query_repo
            .search_text_bodies_within_budget(space_id, candidates, max_bytes)
            .await?)
    }

    pub(crate) async fn has_children_many(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> SearchResult<HashMap<Uuid, bool>> {
        Ok(self
            .authority_repo
            .has_children_many(space_id, node_ids)
            .await?)
    }

    pub(crate) async fn text_stats_many(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> SearchResult<HashMap<Uuid, TextStats>> {
        Ok(self.query_repo.text_stats_many(space_id, node_ids).await?)
    }

    pub(crate) async fn file_stats_many(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> SearchResult<HashMap<Uuid, FileStats>> {
        Ok(self.query_repo.file_stats_many(space_id, node_ids).await?)
    }

    pub(crate) async fn direct_write_lock_ancestors_many(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> SearchResult<HashMap<Uuid, Vec<(Uuid, String, bool)>>> {
        Ok(self
            .authority_repo
            .direct_write_lock_ancestors_many(space_id, node_ids)
            .await?)
    }

    pub(crate) async fn node_paths_many(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> SearchResult<HashMap<Uuid, String>> {
        Ok(self
            .authority_repo
            .node_paths_many(space_id, node_ids)
            .await?)
    }
}
