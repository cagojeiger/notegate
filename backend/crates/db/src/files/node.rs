use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::FilesRepo;
use super::error::map_sqlx_error;
use super::rows::NodeRow;
use notegate_domain::files::{FilesError, FilesResult, Node};

impl FilesRepo {
    pub(super) async fn child_nodes(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
    ) -> FilesResult<Vec<Node>> {
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
              AND n.parent_id = $2
              AND n.deleted_at IS NULL
            ORDER BY n.sort_order, n.name
            "#,
        )
        .bind(workspace_id)
        .bind(parent_node_id)
        .fetch_all(self.pool())
        .await
        .map_err(map_sqlx_error)?
        .into_iter()
        .map(NodeRow::into_node)
        .collect();

        Ok(rows)
    }

    pub(super) async fn create_folder_node(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
        path: &str,
    ) -> FilesResult<Node> {
        let row = sqlx::query_as::<_, NodeRow>(
            r#"
            INSERT INTO nodes (workspace_id, parent_id, name, kind, path_cache)
            VALUES ($1, $2, $3, 'folder', $4)
            RETURNING
                id,
                parent_id,
                name,
                kind,
                path_cache,
                sort_order,
                false AS has_children,
                created_at,
                updated_at
            "#,
        )
        .bind(workspace_id)
        .bind(parent_node_id)
        .bind(name)
        .bind(path)
        .fetch_one(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        Ok(row.into_node())
    }

    pub(super) async fn move_node_record(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        new_parent_node_id: Uuid,
        new_name: &str,
        old_path: &str,
        new_path: &str,
    ) -> FilesResult<()> {
        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;

        sqlx::query(
            r#"
            UPDATE nodes
            SET parent_id = $3,
                name = $4,
                updated_at = now()
            WHERE workspace_id = $1
              AND id = $2
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .bind(new_parent_node_id)
        .bind(new_name)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        update_subtree_paths(&mut tx, workspace_id, node_id, old_path, new_path).await?;
        tx.commit().await.map_err(map_sqlx_error)?;

        Ok(())
    }

    pub(super) async fn soft_delete_subtree(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> FilesResult<()> {
        sqlx::query(
            r#"
            WITH RECURSIVE descendants AS (
                SELECT id
                FROM nodes
                WHERE workspace_id = $1
                  AND id = $2

                UNION ALL

                SELECT n.id
                FROM nodes n
                JOIN descendants d
                  ON n.parent_id = d.id
                WHERE n.workspace_id = $1
                  AND n.deleted_at IS NULL
            )
            UPDATE nodes
            SET deleted_at = now(),
                updated_at = now()
            WHERE workspace_id = $1
              AND id IN (SELECT id FROM descendants)
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .execute(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        Ok(())
    }

    pub(super) async fn node_by_id(&self, workspace_id: Uuid, node_id: Uuid) -> FilesResult<Node> {
        let row = sqlx::query_as::<_, NodeRow>(NODE_SELECT_BY_ID)
            .bind(workspace_id)
            .bind(node_id)
            .fetch_optional(self.pool())
            .await
            .map_err(map_sqlx_error)?;

        row.map(NodeRow::into_node)
            .ok_or_else(|| FilesError::NotFound("node not found".into()))
    }

    pub(super) async fn node_by_path(&self, workspace_id: Uuid, path: &str) -> FilesResult<Node> {
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
              AND n.path_cache = $2
              AND n.deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(path)
        .fetch_optional(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        row.map(NodeRow::into_node)
            .ok_or_else(|| FilesError::NotFound("node not found".into()))
    }
}

async fn update_subtree_paths(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    moving_node_id: Uuid,
    old_prefix: &str,
    new_prefix: &str,
) -> FilesResult<()> {
    sqlx::query(
        r#"
        UPDATE nodes
        SET path_cache = $4 || substring(path_cache from length($3) + 1),
            updated_at = now()
        WHERE workspace_id = $1
          AND deleted_at IS NULL
          AND (
            id = $2
            OR path_cache LIKE $3 || '/%'
          )
        "#,
    )
    .bind(workspace_id)
    .bind(moving_node_id)
    .bind(old_prefix)
    .bind(new_prefix)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;

    Ok(())
}

const NODE_SELECT_BY_ID: &str = r#"
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
"#;
