use uuid::Uuid;

use super::error::map_sqlx_error;
use super::rows::{GrepCandidateRow, NodeRow};
use super::validation::{clamp_limit, normalize_path};
use super::{FilesRepo, FilesRepoError, FilesResult, FindRequest, GrepMatch, GrepRequest, Node};

impl FilesRepo {
    pub async fn find(&self, user_id: Uuid, request: FindRequest) -> FilesResult<Vec<Node>> {
        let q = request.q.trim();
        if q.is_empty() {
            return Err(FilesRepoError::InvalidInput("query cannot be empty".into()));
        }
        if let Some(kind) = request.kind.as_deref() {
            if kind != "folder" && kind != "document" {
                return Err(FilesRepoError::InvalidInput("invalid node kind".into()));
            }
        }

        let workspace_id = self.default_workspace_id(user_id).await?;
        let limit = clamp_limit(request.limit);
        let path = request.path.as_deref().map(normalize_path).transpose()?;
        let like_q = format!("%{q}%");
        let subtree_like = path
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
        .bind(request.kind)
        .bind(path)
        .bind(subtree_like)
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        Ok(rows.into_iter().map(NodeRow::into_node).collect())
    }

    pub async fn grep(&self, user_id: Uuid, request: GrepRequest) -> FilesResult<Vec<GrepMatch>> {
        let q = request.q.trim();
        if q.is_empty() {
            return Err(FilesRepoError::InvalidInput("query cannot be empty".into()));
        }

        let workspace_id = self.default_workspace_id(user_id).await?;
        let limit = clamp_limit(request.limit) as usize;
        let context = request.context.unwrap_or(0).clamp(0, 5) as usize;
        let path = request.path.as_deref().map(normalize_path).transpose()?;
        let subtree_like = path
            .as_ref()
            .map(|p| format!("{}/%", p.trim_end_matches('/')));
        let like_q = format!("%{q}%");

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
        .bind(path)
        .bind(subtree_like)
        .bind(limit as i64)
        .fetch_all(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        let needle = q.to_lowercase();
        let mut matches = Vec::new();
        for candidate in candidates {
            let lines: Vec<&str> = candidate.content_md.split('\n').collect();
            for (idx, line) in lines.iter().enumerate() {
                if !line.to_lowercase().contains(&needle) {
                    continue;
                }

                let before_start = idx.saturating_sub(context);
                let before = lines[before_start..idx]
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect();
                let after_end = (idx + 1 + context).min(lines.len());
                let after = lines[idx + 1..after_end]
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect();

                matches.push(GrepMatch {
                    node_id: candidate.node_id,
                    path: candidate.path_cache.clone(),
                    line_no: idx as i64 + 1,
                    line: (*line).to_owned(),
                    before,
                    after,
                });

                if matches.len() >= limit {
                    return Ok(matches);
                }
            }
        }

        Ok(matches)
    }
}
