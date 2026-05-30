mod create;
mod delete;
mod move_node;
mod save;
mod workspace;

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::error::map_sqlx_error;
use super::rows::NodeRow;
use notegate_domain::files::{FilesError, FilesResult, Node, NodeKind};

pub(super) async fn default_workspace_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> FilesResult<Uuid> {
    let workspace_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM workspaces
        WHERE owner_user_id = $1
          AND name = 'default'
        "#,
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;

    workspace_id.ok_or_else(|| FilesError::NotFound("default workspace not found".into()))
}

pub(super) async fn lock_workspace_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
) -> FilesResult<()> {
    sqlx::query(
        r#"
        SELECT pg_advisory_xact_lock(hashtext($1::TEXT), 0)
        "#,
    )
    .bind(workspace_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;

    Ok(())
}

pub(super) async fn live_node_for_update(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    node_id: Uuid,
) -> FilesResult<Node> {
    let row = sqlx::query_as::<_, NodeRow>(
        r#"
        SELECT
            id,
            parent_id,
            name,
            kind,
            path_cache,
            sort_order,
            false AS has_children,
            created_at,
            updated_at
        FROM nodes
        WHERE workspace_id = $1
          AND id = $2
          AND deleted_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;

    row.map(NodeRow::into_node)
        .ok_or_else(|| FilesError::NotFound("node not found".into()))
}

pub(super) async fn node_by_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    node_id: Uuid,
) -> FilesResult<Node> {
    let row = sqlx::query_as::<_, NodeRow>(
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
          AND n.id = $2
          AND n.deleted_at IS NULL
        "#,
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;

    row.map(NodeRow::into_node)
        .ok_or_else(|| FilesError::NotFound("node not found".into()))
}

pub(super) async fn lock_subtree_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    node_id: Uuid,
) -> FilesResult<()> {
    sqlx::query(
        r#"
        WITH RECURSIVE subtree AS (
            SELECT id
            FROM nodes
            WHERE workspace_id = $1
              AND id = $2
              AND deleted_at IS NULL

            UNION ALL

            SELECT n.id
            FROM nodes n
            JOIN subtree s
              ON n.parent_id = s.id
            WHERE n.workspace_id = $1
              AND n.deleted_at IS NULL
        )
        SELECT n.id
        FROM nodes n
        JOIN subtree s
          ON s.id = n.id
        ORDER BY n.id
        FOR UPDATE
        "#,
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;

    Ok(())
}

pub(super) fn ensure_folder(node: &Node, message: &str) -> FilesResult<()> {
    if node.kind != NodeKind::Folder {
        return Err(FilesError::InvalidInput(message.into()));
    }
    Ok(())
}

pub(super) fn validate_final_name(kind: &NodeKind, name: &str) -> FilesResult<()> {
    validate_base_name(name)?;
    match kind {
        NodeKind::Folder if name.ends_with(".md") => Err(FilesError::InvalidInput(
            "folder name cannot end with .md".into(),
        )),
        NodeKind::Document if !name.ends_with(".md") => Err(FilesError::InvalidInput(
            "document name must end with .md".into(),
        )),
        _ => Ok(()),
    }
}

pub(super) fn child_path(parent_path: &str, name: &str) -> String {
    if parent_path == "/" {
        format!("/{name}")
    } else {
        format!("{parent_path}/{name}")
    }
}

fn validate_base_name(name: &str) -> FilesResult<()> {
    if name.is_empty() {
        return Err(FilesError::InvalidInput("name cannot be empty".into()));
    }
    if name == "." || name == ".." {
        return Err(FilesError::InvalidInput("invalid name".into()));
    }
    if name.contains('/') {
        return Err(FilesError::InvalidInput("name cannot contain /".into()));
    }
    Ok(())
}
