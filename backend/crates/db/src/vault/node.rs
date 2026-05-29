use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::error::map_sqlx_error;
use super::rows::NodeRow;
use super::validation::{child_path, validate_document_name, validate_folder_name};
use super::{Children, Node, NodeKind, VaultRepo, VaultRepoError, VaultResult};

impl VaultRepo {
    pub async fn children(&self, user_id: Uuid, node_id: Uuid) -> VaultResult<Children> {
        let workspace_id = self.default_workspace_id(user_id).await?;
        let parent = self.node_by_id(workspace_id, node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(VaultRepoError::InvalidInput("node is not a folder".into()));
        }

        let children = sqlx::query_as::<_, NodeRow>(
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
        .bind(node_id)
        .fetch_all(self.pool())
        .await
        .map_err(map_sqlx_error)?
        .into_iter()
        .map(NodeRow::into_node)
        .collect();

        Ok(Children { parent, children })
    }

    pub async fn create_folder(
        &self,
        user_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> VaultResult<Node> {
        validate_folder_name(name)?;
        let workspace_id = self.default_workspace_id(user_id).await?;
        let parent = self.node_by_id(workspace_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(VaultRepoError::InvalidInput(
                "parent is not a folder".into(),
            ));
        }

        let path = child_path(&parent.path, name);
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

    pub async fn move_node(
        &self,
        user_id: Uuid,
        node_id: Uuid,
        new_parent_node_id: Uuid,
        new_name: Option<&str>,
    ) -> VaultResult<Node> {
        let workspace_id = self.default_workspace_id(user_id).await?;
        let node = self.node_by_id(workspace_id, node_id).await?;
        if node.parent_id.is_none() {
            return Err(VaultRepoError::Conflict("root cannot be moved".into()));
        }

        let new_parent = self.node_by_id(workspace_id, new_parent_node_id).await?;
        if new_parent.kind != NodeKind::Folder {
            return Err(VaultRepoError::InvalidInput(
                "new parent is not a folder".into(),
            ));
        }

        let final_name = new_name.unwrap_or(&node.name);
        match node.kind {
            NodeKind::Folder => validate_folder_name(final_name)?,
            NodeKind::Document => validate_document_name(final_name)?,
        }

        if node.id == new_parent.id
            || new_parent.path == node.path
            || new_parent
                .path
                .starts_with(&format!("{}/", node.path.trim_end_matches('/')))
        {
            return Err(VaultRepoError::Conflict(
                "node cannot move into itself or its descendant".into(),
            ));
        }

        let old_path = node.path.clone();
        let new_path = child_path(&new_parent.path, final_name);
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
        .bind(final_name)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        update_subtree_paths(&mut tx, workspace_id, node_id, &old_path, &new_path).await?;
        tx.commit().await.map_err(map_sqlx_error)?;

        self.node_by_id(workspace_id, node_id).await
    }

    pub async fn delete_node(&self, user_id: Uuid, node_id: Uuid) -> VaultResult<()> {
        let workspace_id = self.default_workspace_id(user_id).await?;
        let node = self.node_by_id(workspace_id, node_id).await?;
        if node.parent_id.is_none() {
            return Err(VaultRepoError::Conflict("root cannot be deleted".into()));
        }

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

    pub(super) async fn node_by_id(&self, workspace_id: Uuid, node_id: Uuid) -> VaultResult<Node> {
        let row = sqlx::query_as::<_, NodeRow>(NODE_SELECT_BY_ID)
            .bind(workspace_id)
            .bind(node_id)
            .fetch_optional(self.pool())
            .await
            .map_err(map_sqlx_error)?;

        row.map(NodeRow::into_node)
            .ok_or_else(|| VaultRepoError::NotFound("node not found".into()))
    }

    pub(super) async fn node_by_path(&self, workspace_id: Uuid, path: &str) -> VaultResult<Node> {
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
            .ok_or_else(|| VaultRepoError::NotFound("node not found".into()))
    }
}

async fn update_subtree_paths(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    moving_node_id: Uuid,
    old_prefix: &str,
    new_prefix: &str,
) -> VaultResult<()> {
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
