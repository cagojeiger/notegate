//! Search queries: `find` (node name metadata) and `grep` (document content
//! candidates), both optionally scoped to a path's subtree.
//!
//! There is no stored path. A workspace-bounded recursive CTE (`tree`) derives
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
use notegate_service::search::{FindCursor, GrepCandidate, GrepCursor};

/// The recursive CTE that derives every live node's absolute path top-down.
/// Yields `(id, path)` where the root is `/` and a child is `parent || '/' ||
/// name` (root's children become `/name`, not `//name`).
const TREE_CTE: &str = "WITH RECURSIVE tree AS ( \
        SELECT id, parent_id, '/' AS path \
        FROM nodes \
        WHERE workspace_id = $1 AND parent_id IS NULL AND deleted_at IS NULL \
        UNION ALL \
        SELECT n.id, n.parent_id, \
               CASE WHEN t.path = '/' THEN '/' || n.name ELSE t.path || '/' || n.name END \
        FROM nodes n \
        JOIN tree t ON n.parent_id = t.id \
        WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
     )";

/// Resolve a scope path (e.g. `/projects/notes`) to a live folder node id within
/// the workspace, or `None` if it does not resolve to a live node. The root path
/// (`/` or empty) resolves to the workspace root.
pub async fn resolve_scope_node(
    pool: &PgPool,
    workspace_id: Uuid,
    scope_path: &str,
) -> Result<Option<Uuid>> {
    let trimmed = scope_path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM nodes \
             WHERE workspace_id = $1 AND parent_id IS NULL AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)?;
        return Ok(id);
    }

    // Walk segments from the root, resolving each `(parent_id, name)` step.
    let mut current: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM nodes \
         WHERE workspace_id = $1 AND parent_id IS NULL AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    for segment in trimmed.split('/').filter(|s| !s.is_empty()) {
        let Some(parent) = current else {
            return Ok(None);
        };
        current = sqlx::query_scalar(
            "SELECT id FROM nodes \
             WHERE workspace_id = $1 AND parent_id = $2 AND name = $3 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
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
    workspace_id: Uuid,
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
    // ($NULL means "whole workspace"); cursor params follow. `has_children` is
    // a correlated EXISTS so the result is a complete node view in one round-trip.
    let base = format!(
        "{TREE_CTE}, scope AS ( \
            SELECT id FROM nodes \
            WHERE workspace_id = $1 AND ($2::uuid IS NULL OR id = $2) AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n \
            JOIN scope s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT {cols}, t.path AS derived_path, \
                EXISTS ( \
                  SELECT 1 FROM nodes c \
                  WHERE c.workspace_id = $1 AND c.parent_id = n.id AND c.deleted_at IS NULL \
                ) AS has_children \
         FROM nodes n \
         JOIN tree t ON t.id = n.id \
         WHERE n.workspace_id = $1 \
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
                .bind(workspace_id)
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
            .bind(workspace_id)
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

/// Fetch grep candidate documents whose content matches `q` (ILIKE), optionally
/// restricted to the subtree of `scope_node_id`. Ordered by `(updated_at DESC,
/// node_id)` to use `documents_workspace_updated_idx`. The caller passes the exact
/// fetch size, including any lookahead. Each row carries the derived path and
/// content for service-side line splitting.
pub async fn grep_candidates(
    pool: &PgPool,
    workspace_id: Uuid,
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
            WHERE workspace_id = $1 AND ($2::uuid IS NULL OR id = $2) AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n \
            JOIN scope s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT d.node_id, t.path AS derived_path, d.content_md, d.updated_at \
         FROM documents d \
         JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
         JOIN tree t ON t.id = d.node_id \
         WHERE d.workspace_id = $1 \
           AND n.deleted_at IS NULL \
           AND ($2::uuid IS NULL OR d.node_id IN (SELECT id FROM scope)) \
           AND d.content_md ILIKE $3"
    );

    let rows = match cursor {
        None => {
            sqlx::query_as::<_, GrepRow>(&format!(
                "{base} ORDER BY d.updated_at DESC, d.node_id LIMIT $4"
            ))
            .bind(workspace_id)
            .bind(scope_node_id)
            .bind(&pattern)
            .bind(fetch)
            .fetch_all(pool)
            .await
        }
        Some(cursor) => {
            // Keyset over a DESC primary key. The cursor's own document is
            // INCLUDED (`node_id >= …`) so grep can resume mid-document: the
            // service skips `match_offset` already-emitted matches in it. When a
            // document is fully consumed the cursor advances to the next document
            // with `match_offset = 0`, so including it and skipping 0 is correct.
            sqlx::query_as::<_, GrepRow>(&format!(
                "{base} AND (d.updated_at < $4 OR (d.updated_at = $4 AND d.node_id >= $5)) \
                 ORDER BY d.updated_at DESC, d.node_id LIMIT $6"
            ))
            .bind(workspace_id)
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
            content_md: row.content_md,
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
    content_md: String,
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
