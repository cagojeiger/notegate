//! Node reads, counts, depth/subtree/ancestor checks, and path derivation.
//!
//! Nodes have no stored path. The display path is derived by a recursive CTE
//! that walks the parent chain (bounded by `max_path_depth = 5`).
//! All reads exclude soft-deleted rows unless the function name says otherwise.

use notegate_core::{Error, Result};
use notegate_model::{Node, Role};
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::map_sqlx_error;
use super::super::rows::{NODE_COLUMNS, NodeRow};

/// The caller's live (non-revoked) workspace role, or `None` if no live grant.
/// Shared by the [`FilesStore`](notegate_service::files::FilesStore) and
/// [`SearchStore`](notegate_service::search::SearchStore) authorization paths.
pub async fn role_for(pool: &PgPool, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM workspace_access \
         WHERE workspace_id = $1 AND account_id = $2 AND revoked_at IS NULL",
    )
    .bind(workspace_id)
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    role.map(|value| {
        Role::parse(&value)
            .ok_or_else(|| Error::internal(format!("unknown workspace role: {value}")))
    })
    .transpose()
}

/// The workspace's canonical root node (`parent_id IS NULL`).
pub async fn root_node(pool: &PgPool, workspace_id: Uuid) -> Result<Node> {
    let row = sqlx::query_as::<_, NodeRow>(&format!(
        "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE workspace_id = $1 AND parent_id IS NULL"
    ))
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    row.into_node()
}

/// Load a live node by id within a workspace.
pub async fn find_node(pool: &PgPool, workspace_id: Uuid, node_id: Uuid) -> Result<Option<Node>> {
    let row = sqlx::query_as::<_, NodeRow>(&format!(
        "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;
    row.map(NodeRow::into_node).transpose()
}

/// Load a soft-deleted node by id (used by `restore`).
pub async fn find_deleted_node(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
) -> Result<Option<Node>> {
    let row = sqlx::query_as::<_, NodeRow>(&format!(
        "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NOT NULL"
    ))
    .bind(workspace_id)
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
pub async fn node_path(pool: &PgPool, workspace_id: Uuid, node_id: Uuid) -> Result<Option<String>> {
    derive_path(pool, workspace_id, node_id).await
}

/// Shared path-derivation CTE used by `node_path` and search result assembly.
/// Returns the absolute path of a live node, or `None` if it is missing/deleted.
pub async fn derive_path(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
) -> Result<Option<String>> {
    let path: Option<String> = sqlx::query_scalar(
        "WITH RECURSIVE chain AS ( \
            SELECT id, parent_id, name, 0 AS depth \
            FROM nodes \
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id, n.parent_id, n.name, c.depth + 1 \
            FROM nodes n \
            JOIN chain c ON n.id = c.parent_id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT CASE \
                  WHEN max(depth) = 0 THEN '/' \
                  ELSE string_agg(name, '/' ORDER BY depth DESC) \
                       FILTER (WHERE parent_id IS NOT NULL) \
                END \
         FROM chain",
    )
    .bind(workspace_id)
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
pub async fn has_children(pool: &PgPool, workspace_id: Uuid, node_id: Uuid) -> Result<bool> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
            SELECT 1 FROM nodes \
            WHERE workspace_id = $1 AND parent_id = $2 AND deleted_at IS NULL \
         )",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    Ok(exists)
}

/// Count of live direct children of a folder.
pub async fn count_live_children(
    pool: &PgPool,
    workspace_id: Uuid,
    parent_node_id: Uuid,
) -> Result<usize> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nodes \
         WHERE workspace_id = $1 AND parent_id = $2 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(parent_node_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    to_usize(count, "child")
}

/// A live direct child of `parent_node_id` with the given name, if any.
pub async fn find_live_child_by_name(
    pool: &PgPool,
    workspace_id: Uuid,
    parent_node_id: Uuid,
    name: &str,
) -> Result<Option<Node>> {
    let row = sqlx::query_as::<_, NodeRow>(&format!(
        "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE workspace_id = $1 AND parent_id = $2 AND name = $3 AND deleted_at IS NULL"
    ))
    .bind(workspace_id)
    .bind(parent_node_id)
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;
    row.map(NodeRow::into_node).transpose()
}

/// Count of live nodes in a workspace.
pub async fn count_live_nodes(pool: &PgPool, workspace_id: Uuid) -> Result<usize> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nodes WHERE workspace_id = $1 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    to_usize(count, "node")
}

/// A page of live direct children, keyset-ordered by `(sort_order, name, id)`.
/// Fetches `limit + 1` rows to detect whether more follow.
pub async fn paged_children(
    pool: &PgPool,
    workspace_id: Uuid,
    parent_node_id: Uuid,
    limit: i64,
    cursor: Option<(i32, &str, Uuid)>,
) -> Result<(Vec<Node>, bool)> {
    let fetch = limit + 1;
    let rows: Vec<NodeRow> = match cursor {
        None => {
            sqlx::query_as::<_, NodeRow>(&format!(
                "SELECT {NODE_COLUMNS} FROM nodes \
                 WHERE workspace_id = $1 AND parent_id = $2 AND deleted_at IS NULL \
                 ORDER BY sort_order, name, id \
                 LIMIT $3"
            ))
            .bind(workspace_id)
            .bind(parent_node_id)
            .bind(fetch)
            .fetch_all(pool)
            .await
        }
        Some((sort_order, name, id)) => {
            sqlx::query_as::<_, NodeRow>(&format!(
                "SELECT {NODE_COLUMNS} FROM nodes \
                 WHERE workspace_id = $1 AND parent_id = $2 AND deleted_at IS NULL \
                   AND (sort_order, name, id) > ($3, $4, $5) \
                 ORDER BY sort_order, name, id \
                 LIMIT $6"
            ))
            .bind(workspace_id)
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
/// (≤ `workspace_max_nodes`).
pub async fn subtree_relative_depth(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
) -> Result<usize> {
    let max_depth: i32 = sqlx::query_scalar(
        "WITH RECURSIVE subtree AS ( \
            SELECT id, 0 AS depth \
            FROM nodes \
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id, s.depth + 1 \
            FROM nodes n \
            JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT COALESCE(max(depth), 0) FROM subtree",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    to_usize(i64::from(max_depth), "depth")
}

/// Count of live nodes in the subtree rooted at `node_id` (including itself).
pub async fn subtree_live_count(pool: &PgPool, workspace_id: Uuid, node_id: Uuid) -> Result<usize> {
    let count: i64 = sqlx::query_scalar(
        "WITH RECURSIVE subtree AS ( \
            SELECT id \
            FROM nodes \
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id \
            FROM nodes n \
            JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT count(*) FROM subtree",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    to_usize(count, "subtree node")
}

/// Whether `candidate_id` is `node_id` itself or any live descendant of it.
pub async fn is_self_or_descendant(
    pool: &PgPool,
    workspace_id: Uuid,
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
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id \
            FROM nodes n \
            JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT EXISTS (SELECT 1 FROM subtree WHERE id = $3)",
    )
    .bind(workspace_id)
    .bind(node_id)
    .bind(candidate_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    Ok(found)
}

/// Whether any ancestor of `node_id` is currently soft-deleted. Walks the parent
/// chain upward without the live filter, checking each ancestor's `deleted_at`.
pub async fn has_deleted_ancestor(
    pool: &PgPool,
    workspace_id: Uuid,
    node_id: Uuid,
) -> Result<bool> {
    let found: bool = sqlx::query_scalar(
        "WITH RECURSIVE chain AS ( \
            SELECT id, parent_id, deleted_at, 0 AS depth \
            FROM nodes \
            WHERE workspace_id = $1 AND id = $2 \
            UNION ALL \
            SELECT n.id, n.parent_id, n.deleted_at, c.depth + 1 \
            FROM nodes n \
            JOIN chain c ON n.id = c.parent_id \
            WHERE n.workspace_id = $1 \
         ) \
         SELECT EXISTS ( \
            SELECT 1 FROM chain WHERE depth > 0 AND deleted_at IS NOT NULL \
         )",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    Ok(found)
}

/// Convert a non-negative `count(*)`/`max(depth)` to `usize`, erroring on a
/// negative value (impossible, but checked instead of silently wrapping).
fn to_usize(value: i64, label: &str) -> Result<usize> {
    usize::try_from(value)
        .map_err(|_error| notegate_core::Error::internal(format!("negative {label} count")))
}
