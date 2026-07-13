//! Read queries for the file tree.

pub mod text {
    //! Text reads: load live text content and metrics.

    use notegate_core::Result;
    use notegate_model::files::TextStats;
    use notegate_model::{Node, TextObject};
    use sqlx::PgPool;
    use std::collections::HashMap;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;
    use super::super::rows::{NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};

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

    /// Load live text metrics for a bounded set of node ids.
    pub async fn text_stats_many(
        pool: &PgPool,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, TextStats>> {
        if node_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let rows: Vec<(Uuid, String, i64, i32)> = sqlx::query_as(
            "SELECT d.node_id, d.content_sha256, d.byte_len, d.line_count \
             FROM text_objects d \
             JOIN nodes n ON n.id = d.node_id AND n.space_id = d.space_id \
             WHERE d.space_id = $1 \
               AND d.node_id = ANY($2) \
               AND n.deleted_at IS NULL \
               AND n.kind = 'text'",
        )
        .bind(space_id)
        .bind(node_ids.to_vec())
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|(node_id, content_sha256, byte_len, line_count)| {
                (
                    node_id,
                    TextStats {
                        content_sha256,
                        byte_len,
                        line_count,
                    },
                )
            })
            .collect())
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
}

pub mod file {
    //! File reads: metadata stats, file object lookup, and inline bytes.

    use notegate_core::{Error, Result};
    use notegate_model::files::FileStats;
    use notegate_model::{FileObject, Node};
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;
    use super::super::rows::{FILE_COLUMNS, FileRow, NODE_COLUMNS, NodeRow};

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

    pub async fn file_stats_many(
        pool: &PgPool,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, FileStats>> {
        if node_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let columns = FILE_COLUMNS
            .split(',')
            .map(|c| format!("f.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let rows: Vec<FileRow> = sqlx::query_as::<_, FileRow>(&format!(
            "SELECT {columns} FROM file_objects f \
             JOIN nodes n ON n.id = f.node_id AND n.space_id = f.space_id \
             WHERE f.space_id = $1 \
               AND f.node_id = ANY($2) \
               AND n.deleted_at IS NULL \
               AND n.kind = 'file'"
        ))
        .bind(space_id)
        .bind(node_ids.to_vec())
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)?;

        let mut stats = std::collections::HashMap::with_capacity(rows.len());
        for row in rows {
            let file = row.into_file()?;
            stats.insert(
                file.node_id,
                FileStats {
                    storage_kind: file.storage_kind,
                    media_type: file.media_type,
                    byte_len: file.byte_len,
                    content_sha256: file.content_sha256,
                    original_filename: file.original_filename,
                    encryption_mode: file.encryption_mode,
                    encryption_metadata: file.encryption_metadata,
                },
            );
        }
        Ok(stats)
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
}

pub mod node {
    //! Node reads, counts, depth/subtree/ancestor checks, and path derivation.
    //!
    //! Nodes have no stored path. The display path is derived by a recursive CTE
    //! that walks the parent chain (bounded by `max_path_depth = 7`).
    //! All reads exclude soft-deleted rows unless the function name says otherwise.

    use chrono::{DateTime, Utc};
    use notegate_core::{Error, Result};
    use notegate_model::files::NodeListSort;
    use notegate_model::{Node, NodeKind};
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

    /// Derived absolute display paths for a bounded set of live nodes.
    pub async fn node_paths_many(
        pool: &PgPool,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, String>> {
        if node_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "WITH RECURSIVE chain AS ( \
                SELECT id AS target_id, id, parent_id, name, 0 AS depth \
                FROM nodes \
                WHERE space_id = $1 AND id = ANY($2) AND deleted_at IS NULL \
                UNION ALL \
                SELECT c.target_id, n.id, n.parent_id, n.name, c.depth + 1 \
                FROM nodes n \
                JOIN chain c ON n.id = c.parent_id \
                WHERE n.space_id = $1 AND n.deleted_at IS NULL \
             ) \
             SELECT target_id, \
                    CASE WHEN max(depth) = 0 THEN '/' \
                         ELSE '/' || string_agg(name, '/' ORDER BY depth DESC) \
                              FILTER (WHERE parent_id IS NOT NULL) \
                    END AS path \
             FROM chain \
             GROUP BY target_id",
        )
        .bind(space_id)
        .bind(node_ids.to_vec())
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(rows.into_iter().collect())
    }

    /// Live ancestor chain from root to `node_id`, including the target.
    ///
    /// Returns an empty vector when the target is missing or soft-deleted. The
    /// caller can use the result to reveal a node in a lazily loaded tree.
    pub async fn ancestor_chain(pool: &PgPool, space_id: Uuid, node_id: Uuid) -> Result<Vec<Node>> {
        let rows: Vec<NodeRow> = sqlx::query_as::<_, NodeRow>(&format!(
            "WITH RECURSIVE chain AS ( \
                SELECT {NODE_COLUMNS}, 0 AS depth \
                FROM nodes \
                WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
                UNION ALL \
                SELECT n.id, n.space_id, n.parent_id, n.name, n.kind, n.sort_order, n.metadata, \
                       n.created_by_account_id, n.updated_by_account_id, n.deleted_by_account_id, \
                       n.purge_after, n.created_at, n.updated_at, n.deleted_at, c.depth + 1 AS depth \
                FROM nodes n \
                JOIN chain c ON n.id = c.parent_id \
                WHERE n.space_id = $1 AND n.deleted_at IS NULL \
            ) \
            SELECT {NODE_COLUMNS} FROM chain ORDER BY depth DESC"
        ))
        .bind(space_id)
        .bind(node_id)
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)?;

        rows.into_iter().map(NodeRow::into_node).collect()
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

    /// Whether each node in a bounded set has any live direct children.
    pub async fn has_children_many(
        pool: &PgPool,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, bool>> {
        if node_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT parent_id::uuid \
             FROM nodes \
             WHERE space_id = $1 \
               AND parent_id = ANY($2) \
               AND deleted_at IS NULL \
             GROUP BY parent_id",
        )
        .bind(space_id)
        .bind(node_ids.to_vec())
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(rows.into_iter().map(|(node_id,)| (node_id, true)).collect())
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

    /// A page of live non-root nodes in a space, ordered for list-style views.
    ///
    /// This is intentionally space-wide and does not replace `paged_children`,
    /// which remains the tree/navigation query.
    pub async fn paged_nodes(
        pool: &PgPool,
        space_id: Uuid,
        kind: Option<NodeKind>,
        sort: NodeListSort,
        limit: i64,
        cursor: Option<NodeListDbCursor<'_>>,
    ) -> Result<(Vec<Node>, bool)> {
        let fetch = limit + 1;
        let kind = kind.map(|kind| kind.as_str().to_owned());

        let order_by = match sort {
            NodeListSort::UpdatedAtDesc => "updated_at DESC, id DESC",
            NodeListSort::NameAsc => "name, id",
        };
        let cursor_predicate = match (sort, cursor) {
            (NodeListSort::UpdatedAtDesc, None) | (NodeListSort::NameAsc, None) => "",
            (NodeListSort::UpdatedAtDesc, Some(NodeListDbCursor::UpdatedAtDesc { .. })) => {
                "AND (updated_at, id) < ($3, $4) "
            }
            (NodeListSort::NameAsc, Some(NodeListDbCursor::NameAsc { .. })) => {
                "AND (name, id) > ($3, $4) "
            }
            _ => return Err(Error::internal("node list cursor sort mismatch")),
        };
        let limit_placeholder = if cursor.is_some() { "$5" } else { "$3" };

        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM nodes \
             WHERE space_id = $1 \
               AND deleted_at IS NULL \
               AND parent_id IS NOT NULL \
               AND ($2::text IS NULL OR kind = $2) \
               {cursor_predicate}\
             ORDER BY {order_by} \
             LIMIT {limit_placeholder}"
        );
        let query = sqlx::query_as::<_, NodeRow>(&sql)
            .bind(space_id)
            .bind(kind.as_deref());

        let rows: Vec<NodeRow> = match cursor {
            None => query.bind(fetch).fetch_all(pool).await,
            Some(NodeListDbCursor::UpdatedAtDesc { updated_at, id }) => {
                query
                    .bind(updated_at)
                    .bind(id)
                    .bind(fetch)
                    .fetch_all(pool)
                    .await
            }
            Some(NodeListDbCursor::NameAsc { name, id }) => {
                query.bind(name).bind(id).bind(fetch).fetch_all(pool).await
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

    #[derive(Debug, Clone, Copy)]
    pub enum NodeListDbCursor<'a> {
        UpdatedAtDesc { updated_at: DateTime<Utc>, id: Uuid },
        NameAsc { name: &'a str, id: Uuid },
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
    //! Path scope resolution and subtree candidate scans for file-tree commands.

    use chrono::{DateTime, Utc};
    use notegate_core::Result;
    use notegate_model::search::{SearchNodeCandidate, SearchTextCandidate};
    use serde_json::Value;
    use sqlx::FromRow;
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;
    use super::super::rows::{NodeRow, TextRow};

    #[derive(Debug, FromRow)]
    struct NodeCandidateRow {
        id: Uuid,
        space_id: Uuid,
        parent_id: Option<Uuid>,
        name: String,
        kind: String,
        sort_order: i32,
        metadata: Value,
        created_by_account_id: Uuid,
        updated_by_account_id: Uuid,
        deleted_by_account_id: Option<Uuid>,
        purge_after: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        deleted_at: Option<DateTime<Utc>>,
        path: String,
        sort_path: String,
    }

    impl NodeCandidateRow {
        fn node_row(&self) -> NodeRow {
            NodeRow {
                id: self.id,
                space_id: self.space_id,
                parent_id: self.parent_id,
                name: self.name.clone(),
                kind: self.kind.clone(),
                sort_order: self.sort_order,
                metadata: self.metadata.clone(),
                created_by_account_id: self.created_by_account_id,
                updated_by_account_id: self.updated_by_account_id,
                deleted_by_account_id: self.deleted_by_account_id,
                purge_after: self.purge_after,
                created_at: self.created_at,
                updated_at: self.updated_at,
                deleted_at: self.deleted_at,
            }
        }

        fn into_candidate(self) -> Result<SearchNodeCandidate> {
            Ok(SearchNodeCandidate {
                node: self.node_row().into_node()?,
                path: self.path,
                sort_path: self.sort_path,
            })
        }
    }

    #[derive(Debug, FromRow)]
    struct TextCandidateRow {
        id: Uuid,
        space_id: Uuid,
        parent_id: Option<Uuid>,
        name: String,
        kind: String,
        sort_order: i32,
        metadata: Value,
        created_by_account_id: Uuid,
        updated_by_account_id: Uuid,
        deleted_by_account_id: Option<Uuid>,
        purge_after: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        deleted_at: Option<DateTime<Utc>>,
        path: String,
        sort_path: String,
        text_content: Option<String>,
        text_encrypted_payload: Option<Value>,
        text_content_sha256: String,
        text_byte_len: i64,
        text_line_count: i32,
        text_media_type: String,
        text_encoding: String,
        text_storage_format: String,
        text_created_by_account_id: Uuid,
        text_updated_by_account_id: Uuid,
        text_created_at: DateTime<Utc>,
        text_updated_at: DateTime<Utc>,
    }

    impl TextCandidateRow {
        fn into_candidate(self) -> Result<SearchTextCandidate> {
            let node = NodeRow {
                id: self.id,
                space_id: self.space_id,
                parent_id: self.parent_id,
                name: self.name,
                kind: self.kind,
                sort_order: self.sort_order,
                metadata: self.metadata,
                created_by_account_id: self.created_by_account_id,
                updated_by_account_id: self.updated_by_account_id,
                deleted_by_account_id: self.deleted_by_account_id,
                purge_after: self.purge_after,
                created_at: self.created_at,
                updated_at: self.updated_at,
                deleted_at: self.deleted_at,
            }
            .into_node()?;
            let text = TextRow {
                node_id: self.id,
                space_id: self.space_id,
                content: self.text_content,
                encrypted_payload: self.text_encrypted_payload,
                content_sha256: self.text_content_sha256,
                byte_len: self.text_byte_len,
                line_count: self.text_line_count,
                media_type: self.text_media_type,
                encoding: self.text_encoding,
                storage_format: self.text_storage_format,
                created_by_account_id: self.text_created_by_account_id,
                updated_by_account_id: self.text_updated_by_account_id,
                created_at: self.text_created_at,
                updated_at: self.text_updated_at,
            }
            .into_text()?;
            Ok(SearchTextCandidate {
                node,
                path: self.path,
                sort_path: self.sort_path,
                text,
            })
        }
    }

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

    pub async fn node_candidates(
        pool: &PgPool,
        space_id: Uuid,
        scope_node_id: Uuid,
        scope_path: &str,
        after_sort_path: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchNodeCandidate>> {
        let rows: Vec<NodeCandidateRow> = sqlx::query_as(&candidate_cte(
            "SELECT id, space_id, parent_id, name, kind, sort_order, metadata, \
                        created_by_account_id, updated_by_account_id, deleted_by_account_id, \
                        purge_after, created_at, updated_at, deleted_at, path, sort_path \
                 FROM subtree \
                 WHERE id <> $2 AND ($4::text IS NULL OR sort_path > $4) \
                 ORDER BY sort_path \
                 LIMIT $5",
        ))
        .bind(space_id)
        .bind(scope_node_id)
        .bind(scope_path)
        .bind(after_sort_path)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)?;

        rows.into_iter()
            .map(NodeCandidateRow::into_candidate)
            .collect()
    }

    pub async fn text_candidates(
        pool: &PgPool,
        space_id: Uuid,
        scope_node_id: Uuid,
        scope_path: &str,
        after_sort_path: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchTextCandidate>> {
        let rows: Vec<TextCandidateRow> = sqlx::query_as(
            &candidate_cte(
                "SELECT s.id, s.space_id, s.parent_id, s.name, s.kind, s.sort_order, s.metadata, \
                        s.created_by_account_id, s.updated_by_account_id, s.deleted_by_account_id, \
                        s.purge_after, s.created_at, s.updated_at, s.deleted_at, s.path, s.sort_path, \
                        t.content_text AS text_content, \
                        t.encrypted_payload AS text_encrypted_payload, \
                        t.content_sha256 AS text_content_sha256, \
                        t.byte_len AS text_byte_len, \
                        t.line_count AS text_line_count, \
                        t.media_type AS text_media_type, \
                        t.encoding AS text_encoding, \
                        t.storage_format AS text_storage_format, \
                        t.created_by_account_id AS text_created_by_account_id, \
                        t.updated_by_account_id AS text_updated_by_account_id, \
                        t.created_at AS text_created_at, \
                        t.updated_at AS text_updated_at \
                 FROM subtree s \
                 JOIN text_objects t ON t.space_id = s.space_id AND t.node_id = s.id \
                 WHERE s.id <> $2 \
                   AND s.kind = 'text' \
                   AND t.storage_format = 'plain' \
                   AND ($4::text IS NULL OR s.sort_path > $4) \
                 ORDER BY s.sort_path \
                 LIMIT $5",
            ),
        )
        .bind(space_id)
        .bind(scope_node_id)
        .bind(scope_path)
        .bind(after_sort_path)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)?;

        rows.into_iter()
            .map(TextCandidateRow::into_candidate)
            .collect()
    }

    fn candidate_cte(select_sql: &str) -> String {
        format!(
            "WITH RECURSIVE subtree AS ( \
                SELECT id, space_id, parent_id, name, kind, sort_order, metadata, \
                       created_by_account_id, updated_by_account_id, deleted_by_account_id, \
                       purge_after, created_at, updated_at, deleted_at, \
                       $3::text AS path, \
                       ''::text AS sort_path \
                FROM nodes \
                WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
                UNION ALL \
                SELECT n.id, n.space_id, n.parent_id, n.name, n.kind, n.sort_order, n.metadata, \
                       n.created_by_account_id, n.updated_by_account_id, n.deleted_by_account_id, \
                       n.purge_after, n.created_at, n.updated_at, n.deleted_at, \
                       CASE WHEN s.path = '/' THEN '/' || n.name ELSE s.path || '/' || n.name END, \
                       CASE WHEN s.sort_path = '' \
                            THEN concat(lpad((n.sort_order::bigint + 2147483648)::text, 10, '0'), E'\\x1f', n.name, E'\\x1f', n.id::text) \
                            ELSE s.sort_path || E'\\x1e' || concat(lpad((n.sort_order::bigint + 2147483648)::text, 10, '0'), E'\\x1f', n.name, E'\\x1f', n.id::text) \
                       END \
                FROM nodes n \
                JOIN subtree s ON n.parent_id = s.id \
                WHERE n.space_id = $1 AND n.deleted_at IS NULL \
            ) \
            {select_sql}"
        )
    }
}
