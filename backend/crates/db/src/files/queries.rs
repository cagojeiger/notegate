//! Read queries for the file tree.
pub mod text {
    //! Text reads: load a live text (node + content), and space-level
    //! text count / total byte sum used by the in-tx capacity checks.

    use notegate_core::Result;
    use notegate_model::files::TextStats;
    use notegate_model::{Node, TextObject};
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;
    use super::super::rows::{NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};
    use crate::to_usize;

    /// Load live text metrics without the Markdown body.
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
    //! Search queries: `find` (node name metadata) and `grep` (text content
    //! candidates), both optionally scoped to a path's subtree.
    //!
    //! There is no stored path. A space-bounded recursive CTE (`tree`) derives
    //! every live node's absolute path top-down (root = `/`, child = parent_path +
    //! name), so commands can return display paths without a stored path column.
    //! `find.q` matches node names only; path is used for scope and display. Scope
    //! is resolved by walking the path segment-by-segment to a node id, then
    //! restricting to that node's subtree.

    use notegate_core::Result;
    use notegate_model::{Node, NodeKind};
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;
    use super::super::rows::{NODE_COLUMNS, NodeRow};
    use notegate_model::search::{FindCursor, GrepCandidate, GrepCursor};

    /// The recursive CTE that derives every live node's absolute path top-down.
    /// Yields `(id, path)` where the root is `/` and a child is `parent || '/' ||
    /// name` (root's children become `/name`, not `//name`).
    const TREE_CTE: &str = "WITH RECURSIVE tree AS ( \
        SELECT id, parent_id, '/' AS path \
        FROM nodes \
        WHERE space_id = $1 AND parent_id IS NULL AND deleted_at IS NULL \
        UNION ALL \
        SELECT n.id, n.parent_id, \
               CASE WHEN t.path = '/' THEN '/' || n.name ELSE t.path || '/' || n.name END \
        FROM nodes n \
        JOIN tree t ON n.parent_id = t.id \
        WHERE n.space_id = $1 AND n.deleted_at IS NULL \
     )";

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

    /// Find live nodes whose name matches `q` (ILIKE), optionally filtered by
    /// `kind` and restricted to the subtree of `scope_node_id`. Keyset ordered by
    /// `(name, id)`. The caller passes the exact fetch size, including any lookahead.
    /// Each row carries the node, its derived path, and whether it has any live
    /// children (so the caller can build a complete node view without an extra
    /// per-row query).
    pub async fn find_nodes(
        pool: &PgPool,
        space_id: Uuid,
        q: &str,
        scope_node_id: Option<Uuid>,
        kind: Option<NodeKind>,
        limit: i64,
        cursor: Option<&FindCursor>,
    ) -> Result<Vec<(Node, String, bool)>> {
        let pattern = like_contains(q);
        let kind_filter = kind.map(|k| k.as_str());
        let fetch = limit;

        // The scope subtree (when present) restricts candidates; `tree` derives the
        // display path returned with each row. `$2` is always the scope node id
        // ($NULL means "whole space"); cursor params follow. `has_children` is
        // a correlated EXISTS so the result is a complete node view in one round-trip.
        let base = format!(
            "{TREE_CTE}, scope AS ( \
            SELECT id FROM nodes \
            WHERE space_id = $1 AND ($2::uuid IS NULL OR id = $2) AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n \
            JOIN scope s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT {cols}, t.path AS derived_path, \
                EXISTS ( \
                  SELECT 1 FROM nodes c \
                  WHERE c.space_id = $1 AND c.parent_id = n.id AND c.deleted_at IS NULL \
                ) AS has_children \
         FROM nodes n \
         JOIN tree t ON t.id = n.id \
         WHERE n.space_id = $1 \
           AND n.deleted_at IS NULL \
           AND n.parent_id IS NOT NULL \
           AND ($2::uuid IS NULL OR n.id IN (SELECT id FROM scope)) \
           AND n.name ILIKE $3 \
           AND ($4::text IS NULL OR n.kind = $4)",
            cols = qualify(NODE_COLUMNS, "n"),
        );

        let rows = match cursor {
            None => {
                sqlx::query_as::<_, FindRow>(&format!("{base} ORDER BY n.name, n.id LIMIT $5"))
                    .bind(space_id)
                    .bind(scope_node_id)
                    .bind(&pattern)
                    .bind(kind_filter)
                    .bind(fetch)
                    .fetch_all(pool)
                    .await
            }
            Some(cursor) => {
                sqlx::query_as::<_, FindRow>(&format!(
                    "{base} AND (n.name, n.id) > ($5, $6) ORDER BY n.name, n.id LIMIT $7"
                ))
                .bind(space_id)
                .bind(scope_node_id)
                .bind(&pattern)
                .bind(kind_filter)
                .bind(&cursor.name)
                .bind(cursor.id)
                .bind(fetch)
                .fetch_all(pool)
                .await
            }
        }
        .map_err(map_sqlx_error)?;

        rows.into_iter()
            .map(|row| Ok((row.node.into_node()?, row.derived_path, row.has_children)))
            .collect()
    }

    /// Fetch grep candidate texts whose content matches `q` (ILIKE), optionally
    /// restricted to the subtree of `scope_node_id`. Ordered by `(updated_at DESC,
    /// node_id)` to use `texts_space_updated_idx`. The caller passes the exact
    /// fetch size, including any lookahead. Each row carries the derived path and
    /// content for service-side line splitting.
    pub async fn grep_candidates(
        pool: &PgPool,
        space_id: Uuid,
        q: &str,
        scope_node_id: Option<Uuid>,
        limit: i64,
        cursor: Option<&GrepCursor>,
    ) -> Result<Vec<GrepCandidate>> {
        let pattern = like_contains(q);
        let fetch = limit;

        let base = format!(
            "{TREE_CTE}, scope AS ( \
            SELECT id FROM nodes \
            WHERE space_id = $1 AND ($2::uuid IS NULL OR id = $2) AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n \
            JOIN scope s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT d.node_id, t.path AS derived_path, d.content_text, d.updated_at \
         FROM text_objects d \
         JOIN nodes n ON n.id = d.node_id AND n.space_id = d.space_id \
         JOIN tree t ON t.id = d.node_id \
         WHERE d.space_id = $1 \
           AND n.deleted_at IS NULL \
           AND ($2::uuid IS NULL OR d.node_id IN (SELECT id FROM scope)) \
           AND d.content_text ILIKE $3"
        );

        let rows = match cursor {
            None => {
                sqlx::query_as::<_, GrepRow>(&format!(
                    "{base} ORDER BY d.updated_at DESC, d.node_id LIMIT $4"
                ))
                .bind(space_id)
                .bind(scope_node_id)
                .bind(&pattern)
                .bind(fetch)
                .fetch_all(pool)
                .await
            }
            Some(cursor) => {
                // Keyset over a DESC primary key. The cursor's own text is
                // INCLUDED (`node_id >= …`) so grep can resume mid-text: the
                // service skips `match_offset` already-emitted matches in it. When a
                // text is fully consumed the cursor advances to the next text
                // with `match_offset = 0`, so including it and skipping 0 is correct.
                sqlx::query_as::<_, GrepRow>(&format!(
                    "{base} AND (d.updated_at < $4 OR (d.updated_at = $4 AND d.node_id >= $5)) \
                 ORDER BY d.updated_at DESC, d.node_id LIMIT $6"
                ))
                .bind(space_id)
                .bind(scope_node_id)
                .bind(&pattern)
                .bind(cursor.updated_at)
                .bind(cursor.node_id)
                .bind(fetch)
                .fetch_all(pool)
                .await
            }
        }
        .map_err(map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|row| GrepCandidate {
                node_id: row.node_id,
                path: row.derived_path,
                content: row.content,
                updated_at: row.updated_at,
            })
            .collect())
    }

    /// A `find` result row: the node columns, its derived path, and whether it has
    /// any live children.
    #[derive(Debug, sqlx::FromRow)]
    struct FindRow {
        #[sqlx(flatten)]
        node: NodeRow,
        derived_path: String,
        has_children: bool,
    }

    /// A `grep` candidate row.
    #[derive(Debug, sqlx::FromRow)]
    struct GrepRow {
        node_id: Uuid,
        derived_path: String,
        content: String,
        updated_at: chrono::DateTime<chrono::Utc>,
    }

    /// Build an ILIKE `%…%` substring pattern, escaping the LIKE metacharacters in
    /// the user query so they match literally.
    fn like_contains(q: &str) -> String {
        let escaped = q
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        format!("%{escaped}%")
    }

    /// Prefix each comma-separated column with `alias.` so a flattened select can
    /// disambiguate the node columns from the joined derived path.
    fn qualify(columns: &str, alias: &str) -> String {
        columns
            .split(',')
            .map(|c| format!("{alias}.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
