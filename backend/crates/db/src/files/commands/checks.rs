//! In-transaction invariant re-enforcement shared by the mutating commands.
//!
//! The DB enforces these inside the mutation transaction so a concurrent writer
//! cannot slip past a structural bound between validation and the write. Space
//! node/content quota is enforced by the locked counter in `space_usage`.

use notegate_core::limits::{self, Limits};
use notegate_core::tier::{UserTier, effective_file_tree_limits};
use notegate_core::{Error, Result};
use sqlx::PgConnection;
use uuid::Uuid;

use super::super::error::map_sqlx_error;
use crate::space_usage::{self, MutationGate};
use crate::{tier_lookup, to_usize};

pub(crate) struct LockedSpace {
    pub gate: MutationGate,
    pub limits: Limits,
    pub owner_tier: UserTier,
    pub default_search_enabled: bool,
    pub default_text_encryption_enabled: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PathBounds {
    pub depth: usize,
    pub bytes: usize,
}

/// Exclude reconciliation, then serialize file-tree mutations in a Space.
/// This closes quota races and keeps reconciliation from observing a partial
/// mutation without making ordinary writes wait for maintenance.
pub(crate) async fn lock_space(tx: &mut PgConnection, space_id: Uuid) -> Result<MutationGate> {
    let gate = space_usage::acquire_mutation_gate(tx, space_id).await?;
    lock_live_space(tx, space_id).await?;
    Ok(gate)
}

/// Lock quota dependencies in account-deletion order: owner, Space, usage.
pub(crate) async fn lock_space_with_limits(
    tx: &mut PgConnection,
    space_id: Uuid,
    base_limits: Limits,
) -> Result<(MutationGate, Limits)> {
    let locked = lock_space_context(tx, space_id, base_limits).await?;
    Ok((locked.gate, locked.limits))
}

pub(crate) async fn lock_space_context(
    tx: &mut PgConnection,
    space_id: Uuid,
    base_limits: Limits,
) -> Result<LockedSpace> {
    let gate = space_usage::acquire_mutation_gate(tx, space_id).await?;
    let tier = tier_lookup::lock_active_space_owner_tier(tx, space_id, "space not found").await?;
    let defaults: Option<(bool, bool)> = sqlx::query_as(
        "SELECT default_search_enabled, default_text_encryption_enabled \
         FROM spaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(space_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let (default_search_enabled, default_text_encryption_enabled) =
        defaults.ok_or_else(|| Error::not_found("space not found"))?;
    Ok(LockedSpace {
        gate,
        limits: effective_file_tree_limits(tier, base_limits),
        owner_tier: tier,
        default_search_enabled,
        default_text_encryption_enabled,
    })
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

/// A live node's identifying fields, fetched inside a transaction. `None` when
/// the node does not exist in the space.
pub struct LiveNode {
    pub kind: String,
    pub name: String,
    pub parent_id: Option<Uuid>,
}

/// Load a live node's identifying fields inside the transaction, or `None`.
pub async fn live_node(
    tx: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<Option<LiveNode>> {
    let row: Option<(String, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT kind, name, parent_id FROM nodes \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(space_id)
    .bind(node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(row.map(|(kind, name, parent_id)| LiveNode {
        kind,
        name,
        parent_id,
    }))
}

/// Assert the parent is a live folder and return its derived path bounds.
pub(crate) async fn require_live_folder_path_bounds(
    tx: &mut PgConnection,
    space_id: Uuid,
    parent_id: Uuid,
) -> Result<PathBounds> {
    let row: Option<(String, i64, i64)> = sqlx::query_as(
        "WITH RECURSIVE chain AS ( \
            SELECT id, parent_id, name, kind AS target_kind, 0::bigint AS depth \
            FROM nodes \
            WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id, n.parent_id, n.name, c.target_kind, c.depth + 1 \
            FROM nodes n JOIN chain c ON n.id = c.parent_id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT target_kind, max(depth)::bigint, \
                CASE WHEN max(depth) = 0 THEN 1::bigint \
                     ELSE 1 \
                          + COALESCE(sum(octet_length(name)) \
                              FILTER (WHERE parent_id IS NOT NULL), 0) \
                          + GREATEST(count(*) FILTER (WHERE parent_id IS NOT NULL) - 1, 0) \
                END::bigint AS path_bytes \
         FROM chain \
         GROUP BY target_kind",
    )
    .bind(space_id)
    .bind(parent_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let Some((kind, depth, bytes)) = row else {
        return Err(Error::not_found("parent node not found"));
    };
    if kind != "folder" {
        return Err(Error::validation("parent must be a folder"));
    }
    Ok(PathBounds {
        depth: to_usize(depth, "depth")?,
        bytes: to_usize(bytes, "path byte length")?,
    })
}

/// Maximum depth and path bytes of a live descendant relative to `node_id`.
pub(crate) async fn subtree_relative_bounds(
    tx: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<PathBounds> {
    let (depth, bytes): (Option<i64>, Option<i64>) = sqlx::query_as(
        "WITH RECURSIVE subtree AS ( \
            SELECT id, 0::bigint AS depth, 0::bigint AS path_bytes \
            FROM nodes WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id, s.depth + 1, s.path_bytes + 1 + octet_length(n.name) \
            FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT max(depth)::bigint, max(path_bytes)::bigint FROM subtree",
    )
    .bind(space_id)
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let (Some(depth), Some(bytes)) = (depth, bytes) else {
        return Err(Error::not_found("node not found"));
    };
    Ok(PathBounds {
        depth: to_usize(depth, "depth")?,
        bytes: to_usize(bytes, "path byte length")?,
    })
}

pub(crate) fn destination_bounds(
    parent: PathBounds,
    name: &str,
    subtree: PathBounds,
) -> Result<PathBounds> {
    let separator_bytes = usize::from(parent.depth > 0);
    let bytes = parent
        .bytes
        .checked_add(separator_bytes)
        .and_then(|value| value.checked_add(name.len()))
        .and_then(|value| value.checked_add(subtree.bytes))
        .ok_or_else(|| Error::internal("path byte length overflow"))?;
    let depth = parent
        .depth
        .checked_add(1)
        .and_then(|value| value.checked_add(subtree.depth))
        .ok_or_else(|| Error::internal("path depth overflow"))?;
    Ok(PathBounds { depth, bytes })
}

pub(crate) fn require_path_limits(bounds: PathBounds) -> Result<()> {
    if bounds.depth > limits::MAX_PATH_DEPTH {
        return Err(Error::validation("path is too deep"));
    }
    if bounds.bytes > limits::MAX_PATH_LEN {
        return Err(Error::validation("path is too long"));
    }
    Ok(())
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
            "folder already has the maximum of {} live children; split into subfolders",
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
