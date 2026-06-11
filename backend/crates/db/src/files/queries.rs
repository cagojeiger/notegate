//! Read queries for the file tree.
pub mod text {
    //! Text reads: load a live text (node + content), and space-level
    //! text count / total byte sum used by the in-tx capacity checks.

    use notegate_core::Result;
    use notegate_model::files::TextStats;
    use notegate_model::{Node, TextObject};
    use sqlx::PgPool;
    use std::collections::HashMap;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;
    use super::super::rows::{NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};
    use crate::to_usize;

    /// Load live text metrics without the content body.
    pub async fn text_stats(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<TextStats>> {
        let row: Option<(String, i64, i32)> = sqlx::query_as(
            "SELECT d.content_sha256, d.byte_len, d.line_count FROM text_objects d \
         JOIN nodes n ON n.id = d.node_id AND n.space_id = d.space_id \
         WHERE d.space_id = $1 AND d.node_id = $2 AND n.deleted_at IS NULL",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(row.map(|(content_sha256, byte_len, line_count)| TextStats {
            content_sha256,
            byte_len,
            line_count,
        }))
    }

    /// Load a live text (its node + content) by node id, or `None` when the node
    /// is missing, soft-deleted, or a folder.
    pub async fn find_text(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<(Node, TextObject)>> {
        let node_row = sqlx::query_as::<_, NodeRow>(&format!(
            "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL AND kind = 'text'"
        ))
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;

        let Some(node_row) = node_row else {
            return Ok(None);
        };

        let doc_row = sqlx::query_as::<_, TextRow>(&format!(
            "SELECT {TEXT_COLUMNS} FROM text_objects \
         WHERE space_id = $1 AND node_id = $2"
        ))
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;

        match doc_row {
            Some(doc_row) => Ok(Some((node_row.into_node()?, doc_row.into_text()?))),
            None => Ok(None),
        }
    }

    /// Load live text objects for a bounded set of node ids.
    ///
    /// The caller already has the live node summaries, so this returns only the
    /// text objects keyed by node id. Missing rows are omitted.
    pub async fn find_texts(
        pool: &PgPool,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, TextObject>> {
        if node_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let columns = TEXT_COLUMNS
            .split(',')
            .map(|c| format!("d.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let rows: Vec<TextRow> = sqlx::query_as::<_, TextRow>(&format!(
            "SELECT {columns} FROM text_objects d \
             JOIN nodes n ON n.id = d.node_id AND n.space_id = d.space_id \
             WHERE d.space_id = $1 \
               AND d.node_id = ANY($2) \
               AND n.deleted_at IS NULL \
               AND n.kind = 'text'"
        ))
        .bind(space_id)
        .bind(node_ids.to_vec())
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)?;

        let mut texts = HashMap::with_capacity(rows.len());
        for row in rows {
            let text = row.into_text()?;
            texts.insert(text.node_id, text);
        }
        Ok(texts)
    }

    /// Count of live texts in a space (joins `text_objects` to live nodes).
    pub async fn count_live_texts(pool: &PgPool, space_id: Uuid) -> Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM text_objects d \
         JOIN nodes n ON n.id = d.node_id AND n.space_id = d.space_id \
         WHERE d.space_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(count, "text")
    }

    /// Sum of `byte_len` over the space's live texts.
    pub async fn sum_live_text_bytes(pool: &PgPool, space_id: Uuid) -> Result<usize> {
        let total: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(d.byte_len), 0)::bigint FROM text_objects d \
         JOIN nodes n ON n.id = d.node_id AND n.space_id = d.space_id \
         WHERE d.space_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(total, "text byte")
    }
}

pub mod file {
    //! File reads: metadata stats, file object lookup, inline bytes, and live file count.

    use notegate_core::{Error, Result};
    use notegate_model::files::FileStats;
    use notegate_model::{FileObject, Node};
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;
    use super::super::rows::{FILE_COLUMNS, FileRow, NODE_COLUMNS, NodeRow};
    use crate::to_usize;

    pub async fn file_stats(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<FileStats>> {
        let columns = FILE_COLUMNS
            .split(',')
            .map(|c| format!("f.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let row: Option<FileRow> = sqlx::query_as::<_, FileRow>(&format!(
            "SELECT {columns} FROM file_objects f \
         JOIN nodes n ON n.id = f.node_id AND n.space_id = f.space_id \
         WHERE f.space_id = $1 AND f.node_id = $2 AND n.deleted_at IS NULL"
        ))
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(|row| {
            let file = row.into_file()?;
            Ok(FileStats {
                storage_kind: file.storage_kind,
                media_type: file.media_type,
                byte_len: file.byte_len,
                content_sha256: file.content_sha256,
                original_filename: file.original_filename,
                encryption_mode: file.encryption_mode,
                encryption_metadata: file.encryption_metadata,
            })
        })
        .transpose()
    }

    pub async fn find_file(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<(Node, FileObject)>> {
        let node_row = sqlx::query_as::<_, NodeRow>(&format!(
            "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL AND kind = 'file'"
        ))
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;

        let Some(node_row) = node_row else {
            return Ok(None);
        };

        let file_row = sqlx::query_as::<_, FileRow>(&format!(
            "SELECT {FILE_COLUMNS} FROM file_objects WHERE space_id = $1 AND node_id = $2"
        ))
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;

        match file_row {
            Some(file_row) => Ok(Some((node_row.into_node()?, file_row.into_file()?))),
            None => Ok(None),
        }
    }

    pub async fn read_inline_file(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<(Node, FileObject, Vec<u8>)>> {
        let Some((node, file)) = find_file(pool, space_id, node_id).await? else {
            return Ok(None);
        };
        let bytes: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT bytes FROM file_inline_contents WHERE space_id = $1 AND node_id = $2",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;

        let bytes = bytes.ok_or_else(|| Error::not_found("file content not found"))?;
        Ok(Some((node, file, bytes)))
    }

    pub async fn count_live_files(pool: &PgPool, space_id: Uuid) -> Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM file_objects f \
         JOIN nodes n ON n.id = f.node_id AND n.space_id = f.space_id \
         WHERE f.space_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(count, "file")
    }
}

pub mod node {
    //! Node reads, counts, depth/subtree/ancestor checks, and path derivation.
    //!
    //! Nodes have no stored path. The display path is derived by a recursive CTE
    //! that walks the parent chain (bounded by `max_path_depth = 5`).
    //! All reads exclude soft-deleted rows unless the function name says otherwise.

    use notegate_core::Result;
    use notegate_model::Node;
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;
    use super::super::rows::{NODE_COLUMNS, NodeRow};
    use crate::to_usize;

    pub async fn permission_for(
        pool: &PgPool,
        space_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<notegate_model::Permission>> {
        crate::space_permission::permission_for(pool, space_id, account_id).await
    }

    /// Load a live node by id within a space.
    pub async fn find_node(pool: &PgPool, space_id: Uuid, node_id: Uuid) -> Result<Option<Node>> {
        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL"
        ))
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;
        row.map(NodeRow::into_node).transpose()
    }

    /// The derived absolute display path of a live node (root = `/`), or `None`.
    ///
    /// Walks the parent chain upward via a recursive CTE, prepending each ancestor's
    /// name. The chain is bounded by the depth limit (≤5 below root) so the recursion
    /// terminates well within Postgres limits.
    pub async fn node_path(pool: &PgPool, space_id: Uuid, node_id: Uuid) -> Result<Option<String>> {
        derive_path(pool, space_id, node_id).await
    }

    /// Shared path-derivation CTE used by `node_path` and search result assembly.
    /// Returns the absolute path of a live node, or `None` if it is missing/deleted.
    pub async fn derive_path(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<String>> {
        let path: Option<String> = sqlx::query_scalar(
            "WITH RECURSIVE chain AS ( \
            SELECT id, parent_id, name, 0 AS depth \
            FROM nodes \
            WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id, n.parent_id, n.name, c.depth + 1 \
            FROM nodes n \
            JOIN chain c ON n.id = c.parent_id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT CASE \
                  WHEN max(depth) = 0 THEN '/' \
                  ELSE string_agg(name, '/' ORDER BY depth DESC) \
                       FILTER (WHERE parent_id IS NOT NULL) \
                END \
         FROM chain",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?
        .flatten();

        // `string_agg` of non-root segments yields e.g. `a/b/c`; prefix the leading
        // slash. The root case already produced exactly `/`.
        Ok(path.map(|p| if p == "/" { p } else { format!("/{p}") }))
    }

    /// Whether a node has any live direct children.
    pub async fn has_children(pool: &PgPool, space_id: Uuid, node_id: Uuid) -> Result<bool> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS ( \
            SELECT 1 FROM nodes \
            WHERE space_id = $1 AND parent_id = $2 AND deleted_at IS NULL \
         )",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(exists)
    }

    /// Count of live direct children of a folder.
    pub async fn count_live_children(
        pool: &PgPool,
        space_id: Uuid,
        parent_node_id: Uuid,
    ) -> Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM nodes \
         WHERE space_id = $1 AND parent_id = $2 AND deleted_at IS NULL",
        )
        .bind(space_id)
        .bind(parent_node_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(count, "child")
    }

    /// A live direct child of `parent_node_id` with the given name, if any.
    pub async fn find_live_child_by_name(
        pool: &PgPool,
        space_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> Result<Option<Node>> {
        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE space_id = $1 AND parent_id = $2 AND name = $3 AND deleted_at IS NULL"
        ))
        .bind(space_id)
        .bind(parent_node_id)
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;
        row.map(NodeRow::into_node).transpose()
    }

    /// Count of live nodes in a space.
    pub async fn count_live_nodes(pool: &PgPool, space_id: Uuid) -> Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM nodes WHERE space_id = $1 AND deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(count, "node")
    }

    /// A page of live direct children, keyset-ordered by `(sort_order, name, id)`.
    /// Fetches `limit + 1` rows to detect whether more follow.
    pub async fn paged_children(
        pool: &PgPool,
        space_id: Uuid,
        parent_node_id: Uuid,
        limit: i64,
        cursor: Option<(i32, &str, Uuid)>,
    ) -> Result<(Vec<Node>, bool)> {
        let fetch = limit + 1;
        let rows: Vec<NodeRow> = match cursor {
            None => {
                sqlx::query_as::<_, NodeRow>(&format!(
                    "SELECT {NODE_COLUMNS} FROM nodes \
                 WHERE space_id = $1 AND parent_id = $2 AND deleted_at IS NULL \
                 ORDER BY sort_order, name, id \
                 LIMIT $3"
                ))
                .bind(space_id)
                .bind(parent_node_id)
                .bind(fetch)
                .fetch_all(pool)
                .await
            }
            Some((sort_order, name, id)) => {
                sqlx::query_as::<_, NodeRow>(&format!(
                    "SELECT {NODE_COLUMNS} FROM nodes \
                 WHERE space_id = $1 AND parent_id = $2 AND deleted_at IS NULL \
                   AND (sort_order, name, id) > ($3, $4, $5) \
                 ORDER BY sort_order, name, id \
                 LIMIT $6"
                ))
                .bind(space_id)
                .bind(parent_node_id)
                .bind(sort_order)
                .bind(name)
                .bind(id)
                .bind(fetch)
                .fetch_all(pool)
                .await
            }
        }
        .map_err(map_sqlx_error)?;

        let has_more = rows.len() as i64 > limit;
        let mut nodes: Vec<Node> = rows
            .into_iter()
            .take(limit as usize)
            .map(NodeRow::into_node)
            .collect::<Result<_>>()?;
        nodes.shrink_to_fit();
        Ok((nodes, has_more))
    }

    /// The maximum depth of any live descendant relative to `node_id` (0 when there
    /// are no live children). Computed by a recursive CTE bounded by the live subtree
    /// (≤ `space_max_nodes`).
    pub async fn subtree_relative_depth(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<usize> {
        let max_depth: i32 = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
            SELECT id, 0 AS depth \
            FROM nodes \
            WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id, s.depth + 1 \
            FROM nodes n \
            JOIN subtree s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT COALESCE(max(depth), 0) FROM subtree",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(i64::from(max_depth), "depth")
    }

    /// Count of live nodes in the subtree rooted at `node_id` (including itself).
    pub async fn subtree_live_count(pool: &PgPool, space_id: Uuid, node_id: Uuid) -> Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
            SELECT id \
            FROM nodes \
            WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id \
            FROM nodes n \
            JOIN subtree s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT count(*) FROM subtree",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(count, "subtree node")
    }

    /// Whether `candidate_id` is `node_id` itself or any live descendant of it.
    pub async fn is_self_or_descendant(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
        candidate_id: Uuid,
    ) -> Result<bool> {
        if node_id == candidate_id {
            return Ok(true);
        }
        let found: bool = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
            SELECT id \
            FROM nodes \
            WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id \
            FROM nodes n \
            JOIN subtree s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT EXISTS (SELECT 1 FROM subtree WHERE id = $3)",
        )
        .bind(space_id)
        .bind(node_id)
        .bind(candidate_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(found)
    }
}

pub mod search {
    //! Path scope resolution for file-tree commands.

    use notegate_core::Result;
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;

    /// Resolve a scope path (e.g. `/projects/notes`) to a live node id within
    /// the space, or `None` if it does not resolve to a live node. The root path
    /// (`/` or empty) resolves to the space root.
    pub async fn resolve_scope_node(
        pool: &PgPool,
        space_id: Uuid,
        scope_path: &str,
    ) -> Result<Option<Uuid>> {
        let trimmed = scope_path.trim();
        if trimmed.is_empty() || trimmed == "/" {
            let id: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM nodes \
             WHERE space_id = $1 AND parent_id IS NULL AND deleted_at IS NULL",
            )
            .bind(space_id)
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx_error)?;
            return Ok(id);
        }

        // Walk segments from the root, resolving each `(parent_id, name)` step.
        let mut current: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM nodes \
         WHERE space_id = $1 AND parent_id IS NULL AND deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;

        for segment in trimmed.split('/').filter(|s| !s.is_empty()) {
            let Some(parent) = current else {
                return Ok(None);
            };
            current = sqlx::query_scalar(
                "SELECT id FROM nodes \
             WHERE space_id = $1 AND parent_id = $2 AND name = $3 AND deleted_at IS NULL",
            )
            .bind(space_id)
            .bind(parent)
            .bind(segment)
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx_error)?;
        }

        Ok(current)
    }
}
