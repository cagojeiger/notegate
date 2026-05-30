use uuid::Uuid;

use super::super::FilesRepo;
use super::super::error::map_sqlx_error;
use super::super::rows::{GrepCandidateRow, NodeRow};
use notegate_domain::files::{FilesResult, FindQuery, GrepCandidate, GrepCandidateQuery, Node};

impl FilesRepo {
    pub(in crate::files) async fn find_nodes(
        &self,
        user_id: Uuid,
        query: FindQuery,
    ) -> FilesResult<Vec<Node>> {
        let workspace_id = self.default_workspace_id(user_id).await?;
        let like_q = format!("%{}%", query.q);
        let kind = query.kind.map(|kind| kind.as_str());
        let subtree_like = query
            .path
            .as_ref()
            .map(|p| format!("{}/%", p.trim_end_matches('/')));

        let rows = sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT
                n.id,
                n.parent_id,
                n.name,
                n.kind,
                n.path_cache,
                n.sort_order,
                EXISTS (
                    SELECT 1
                    FROM nodes c
                    WHERE c.workspace_id = n.workspace_id
                      AND c.parent_id = n.id
                      AND c.deleted_at IS NULL
                ) AS has_children,
                n.created_at,
                n.updated_at
            FROM nodes n
            WHERE n.workspace_id = $1
              AND n.deleted_at IS NULL
              AND n.path_cache ILIKE $2
              AND ($3::TEXT IS NULL OR n.kind = $3)
              AND (
                  $4::TEXT IS NULL
                  OR n.path_cache = $4
                  OR n.path_cache LIKE $5
              )
            ORDER BY n.path_cache
            LIMIT $6
            "#,
        )
        .bind(workspace_id)
        .bind(like_q)
        .bind(kind)
        .bind(query.path)
        .bind(subtree_like)
        .bind(query.limit)
        .fetch_all(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        Ok(rows.into_iter().map(NodeRow::into_node).collect())
    }

    pub(in crate::files) async fn grep_candidates(
        &self,
        user_id: Uuid,
        query: GrepCandidateQuery,
    ) -> FilesResult<Vec<GrepCandidate>> {
        let workspace_id = self.default_workspace_id(user_id).await?;
        let subtree_like = query
            .path
            .as_ref()
            .map(|p| format!("{}/%", p.trim_end_matches('/')));
        let like_q = format!("%{}%", query.q);

        let candidates = sqlx::query_as::<_, GrepCandidateRow>(
            r#"
            SELECT n.id AS node_id, n.path_cache, d.content_md
            FROM documents d
            JOIN nodes n
              ON n.id = d.node_id
             AND n.workspace_id = d.workspace_id
            WHERE d.workspace_id = $1
              AND n.deleted_at IS NULL
              AND d.search_text ILIKE $2
              AND (
                  $3::TEXT IS NULL
                  OR n.path_cache = $3
                  OR n.path_cache LIKE $4
              )
            ORDER BY d.updated_at DESC
            LIMIT $5
            "#,
        )
        .bind(workspace_id)
        .bind(like_q)
        .bind(query.path)
        .bind(subtree_like)
        .bind(query.limit)
        .fetch_all(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        Ok(candidates
            .into_iter()
            .map(GrepCandidateRow::into_candidate)
            .collect())
    }
}
