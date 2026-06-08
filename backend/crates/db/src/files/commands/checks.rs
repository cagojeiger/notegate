//! In-transaction invariant re-enforcement shared by the mutating commands.
//!
//! The service pre-checks these for precise errors; the DB re-checks them inside
//! the mutation's transaction so a concurrent writer cannot slip past a count or
//! depth bound between the pre-check and the write.

use notegate_core::{Error, Result, limits};
use sqlx::PgConnection;
use uuid::Uuid;

use super::super::error::map_sqlx_error;

/// A live node's kind + deleted flag, fetched inside a transaction. `None` when
/// the node does not exist in the workspace.
pub struct LiveNode {
    pub kind: String,
    pub parent_id: Option<Uuid>,
}

/// Load a live node's kind/parent inside the transaction, or `None`.
pub async fn live_node(
    tx: &mut PgConnection,
    workspace_id: Uuid,
    node_id: Uuid,
) -> Result<Option<LiveNode>> {
    let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT kind, parent_id FROM nodes \
         WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(row.map(|(kind, parent_id)| LiveNode { kind, parent_id }))
}

/// Assert the parent is a live folder. Returns its kind error otherwise.
pub async fn require_live_folder(
    tx: &mut PgConnection,
    workspace_id: Uuid,
    parent_id: Uuid,
) -> Result<()> {
    match live_node(tx, workspace_id, parent_id).await? {
        None => Err(Error::not_found("parent node not found")),
        Some(node) if node.kind != "folder" => Err(Error::validation("parent must be a folder")),
        Some(_) => Ok(()),
    }
}

/// Depth of a node below the root (root = 0), computed in-transaction by walking
/// the parent chain upward.
pub async fn node_depth(tx: &mut PgConnection, workspace_id: Uuid, node_id: Uuid) -> Result<usize> {
    let depth: i64 = sqlx::query_scalar(
        "WITH RECURSIVE chain AS ( \
            SELECT id, parent_id, 0 AS depth \
            FROM nodes WHERE workspace_id = $1 AND id = $2 \
            UNION ALL \
            SELECT n.id, n.parent_id, c.depth + 1 \
            FROM nodes n JOIN chain c ON n.id = c.parent_id \
            WHERE n.workspace_id = $1 \
         ) \
         SELECT COALESCE(max(depth), 0)::bigint FROM chain",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    to_usize(depth, "depth")
}

/// Maximum depth of any live descendant relative to `node_id` (0 if none).
pub async fn subtree_relative_depth(
    tx: &mut PgConnection,
    workspace_id: Uuid,
    node_id: Uuid,
) -> Result<usize> {
    let depth: i64 = sqlx::query_scalar(
        "WITH RECURSIVE subtree AS ( \
            SELECT id, 0 AS depth \
            FROM nodes WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id, s.depth + 1 \
            FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT COALESCE(max(depth), 0)::bigint FROM subtree",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    to_usize(depth, "depth")
}

/// Enforce the parent fanout cap (`< FOLDER_MAX_CHILDREN` live children).
pub async fn require_fanout(
    tx: &mut PgConnection,
    workspace_id: Uuid,
    parent_id: Uuid,
) -> Result<()> {
    let children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nodes \
         WHERE workspace_id = $1 AND parent_id = $2 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(parent_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if to_usize(children, "child")? >= limits::FOLDER_MAX_CHILDREN {
        return Err(Error::validation(format!(
            "folder already has the maximum of {} children",
            limits::FOLDER_MAX_CHILDREN
        )));
    }
    Ok(())
}

/// Enforce the workspace live-node cap.
pub async fn require_node_budget(tx: &mut PgConnection, workspace_id: Uuid) -> Result<()> {
    let nodes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nodes WHERE workspace_id = $1 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if to_usize(nodes, "node")? >= limits::WORKSPACE_MAX_NODES {
        return Err(Error::validation(format!(
            "workspace already has the maximum of {} nodes",
            limits::WORKSPACE_MAX_NODES
        )));
    }
    Ok(())
}

/// Enforce the workspace live-document cap.
pub async fn require_document_budget(tx: &mut PgConnection, workspace_id: Uuid) -> Result<()> {
    let docs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM documents d \
         JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
         WHERE d.workspace_id = $1 AND n.deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if to_usize(docs, "document")? >= limits::WORKSPACE_MAX_DOCUMENTS {
        return Err(Error::validation(format!(
            "workspace already has the maximum of {} documents",
            limits::WORKSPACE_MAX_DOCUMENTS
        )));
    }
    Ok(())
}

/// Enforce the workspace total live document-byte budget for a write that
/// replaces `previous_bytes` with `new_bytes` (use `previous_bytes = 0` on
/// create). Errors when the resulting total would exceed the cap.
pub async fn require_byte_budget(
    tx: &mut PgConnection,
    workspace_id: Uuid,
    previous_bytes: i64,
    new_bytes: i64,
) -> Result<()> {
    let total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(d.byte_len), 0)::bigint FROM documents d \
         JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
         WHERE d.workspace_id = $1 AND n.deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let projected = total - previous_bytes + new_bytes;
    if projected > limits::WORKSPACE_MAX_DOCUMENT_BYTES as i64 {
        return Err(Error::validation(format!(
            "write would exceed the workspace document byte budget of {}",
            limits::WORKSPACE_MAX_DOCUMENT_BYTES
        )));
    }
    Ok(())
}

/// Enforce sibling-name uniqueness among live children of `parent_id`, ignoring
/// `ignore_id` (the node being moved/restored, for in-place operations).
pub async fn require_sibling_unique(
    tx: &mut PgConnection,
    workspace_id: Uuid,
    parent_id: Uuid,
    name: &str,
    ignore_id: Option<Uuid>,
) -> Result<()> {
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM nodes \
         WHERE workspace_id = $1 AND parent_id = $2 AND name = $3 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(parent_id)
    .bind(name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    match existing {
        Some(id) if Some(id) != ignore_id => Err(Error::validation(format!(
            "a node named '{name}' already exists in this folder"
        ))),
        _ => Ok(()),
    }
}

fn to_usize(value: i64, label: &str) -> Result<usize> {
    usize::try_from(value).map_err(|_error| Error::internal(format!("negative {label} count")))
}
