//! In-transaction invariant re-enforcement shared by the mutating commands.
//!
//! The DB enforces these inside the mutation transaction so a concurrent writer
//! cannot slip past a structural bound between validation and the write. Space
//! node/content quota is enforced by the locked counter in `space_usage`.

use notegate_core::limits::Limits;
use notegate_core::tier::effective_file_tree_limits;
use notegate_core::{Error, Result};
use sqlx::PgConnection;
use uuid::Uuid;

use super::super::error::map_sqlx_error;
use crate::{space_usage, tier_lookup, to_usize};

/// Exclude reconciliation, then serialize file-tree mutations in a Space.
/// This closes quota races and keeps reconciliation from observing a partial
/// mutation without making ordinary writes wait for maintenance.
pub async fn lock_space(tx: &mut PgConnection, space_id: Uuid) -> Result<()> {
    space_usage::acquire_mutation_gate(tx, space_id).await?;
    lock_live_space(tx, space_id).await
}

/// Lock quota dependencies in account-deletion order: owner, Space, usage.
pub async fn lock_space_with_limits(
    tx: &mut PgConnection,
    space_id: Uuid,
    base_limits: Limits,
) -> Result<Limits> {
    space_usage::acquire_mutation_gate(tx, space_id).await?;
    let tier = tier_lookup::lock_active_space_owner_tier(tx, space_id, "space not found").await?;
    lock_live_space(tx, space_id).await?;
    Ok(effective_file_tree_limits(tier, base_limits))
}

async fn lock_live_space(tx: &mut PgConnection, space_id: Uuid) -> Result<()> {
    let found: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM spaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE")
            .bind(space_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
    if found.is_none() {
        return Err(Error::not_found("space not found"));
    }
    Ok(())
}

/// A live node's kind + deleted flag, fetched inside a transaction. `None` when
/// the node does not exist in the space.
pub struct LiveNode {
    pub kind: String,
    pub parent_id: Option<Uuid>,
}

/// Load a live node's kind/parent inside the transaction, or `None`.
pub async fn live_node(
    tx: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<Option<LiveNode>> {
    let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT kind, parent_id FROM nodes \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(space_id)
    .bind(node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(row.map(|(kind, parent_id)| LiveNode { kind, parent_id }))
}

/// Assert the parent is a live folder. Returns its kind error otherwise.
pub async fn require_live_folder(
    tx: &mut PgConnection,
    space_id: Uuid,
    parent_id: Uuid,
) -> Result<()> {
    match live_node(tx, space_id, parent_id).await? {
        None => Err(Error::not_found("parent node not found")),
        Some(node) if node.kind != "folder" => Err(Error::validation("parent must be a folder")),
        Some(_) => Ok(()),
    }
}

/// Depth of a node below the root (root = 0), computed in-transaction by walking
/// the parent chain upward.
pub async fn node_depth(tx: &mut PgConnection, space_id: Uuid, node_id: Uuid) -> Result<usize> {
    let depth: i64 = sqlx::query_scalar(
        "WITH RECURSIVE chain AS ( \
            SELECT id, parent_id, 0 AS depth \
            FROM nodes WHERE space_id = $1 AND id = $2 \
            UNION ALL \
            SELECT n.id, n.parent_id, c.depth + 1 \
            FROM nodes n JOIN chain c ON n.id = c.parent_id \
            WHERE n.space_id = $1 \
         ) \
         SELECT COALESCE(max(depth), 0)::bigint FROM chain",
    )
    .bind(space_id)
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    to_usize(depth, "depth")
}

/// Maximum depth of any live descendant relative to `node_id` (0 if none).
pub async fn subtree_relative_depth(
    tx: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<usize> {
    let depth: i64 = sqlx::query_scalar(
        "WITH RECURSIVE subtree AS ( \
            SELECT id, 0 AS depth \
            FROM nodes WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id, s.depth + 1 \
            FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT COALESCE(max(depth), 0)::bigint FROM subtree",
    )
    .bind(space_id)
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    to_usize(depth, "depth")
}

/// Enforce the parent fanout cap (`< FOLDER_MAX_CHILDREN` live children).
pub async fn require_fanout(
    tx: &mut PgConnection,
    space_id: Uuid,
    parent_id: Uuid,
    caps: Limits,
) -> Result<()> {
    let children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nodes \
         WHERE space_id = $1 AND parent_id = $2 AND deleted_at IS NULL",
    )
    .bind(space_id)
    .bind(parent_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if to_usize(children, "child")? >= caps.folder_max_children {
        return Err(Error::conflict(format!(
            "folder already has the maximum of {} children",
            caps.folder_max_children
        )));
    }
    Ok(())
}

/// Enforce sibling-name uniqueness among live children of `parent_id`, ignoring
/// `ignore_id` (the node being moved, for in-place operations).
pub async fn require_sibling_unique(
    tx: &mut PgConnection,
    space_id: Uuid,
    parent_id: Uuid,
    name: &str,
    ignore_id: Option<Uuid>,
) -> Result<()> {
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM nodes \
         WHERE space_id = $1 AND parent_id = $2 AND name = $3 AND deleted_at IS NULL",
    )
    .bind(space_id)
    .bind(parent_id)
    .bind(name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    match existing {
        Some(id) if Some(id) != ignore_id => Err(Error::conflict(format!(
            "a node named '{name}' already exists in this folder"
        ))),
        _ => Ok(()),
    }
}
