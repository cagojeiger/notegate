use uuid::Uuid;

use super::error::map_sqlx_error;
use super::rows::NodeRow;
use super::validation::normalize_path;
use super::{Node, VaultRepo, VaultRepoError, VaultResult};

impl VaultRepo {
    pub async fn root(&self, user_id: Uuid) -> VaultResult<Node> {
        let workspace_id = self.initialize_default_workspace(user_id).await?;
        self.root_for_workspace(workspace_id).await
    }

    pub async fn resolve(&self, user_id: Uuid, path: &str) -> VaultResult<Node> {
        let workspace_id = self.default_workspace_id(user_id).await?;
        let path = normalize_path(path)?;
        self.node_by_path(workspace_id, &path).await
    }

    pub(super) async fn default_workspace_id(&self, user_id: Uuid) -> VaultResult<Uuid> {
        let workspace_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id
            FROM workspaces
            WHERE owner_user_id = $1
              AND name = 'default'
            "#,
        )
        .bind(user_id)
        .fetch_optional(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        workspace_id.ok_or_else(|| VaultRepoError::NotFound("default workspace not found".into()))
    }

    pub(super) async fn initialize_default_workspace(&self, user_id: Uuid) -> VaultResult<Uuid> {
        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;
        let inserted_workspace_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO workspaces (owner_user_id, name)
            VALUES ($1, 'default')
            ON CONFLICT (owner_user_id, name) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let workspace_id = match inserted_workspace_id {
            Some(workspace_id) => workspace_id,
            None => sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT id
                FROM workspaces
                WHERE owner_user_id = $1
                  AND name = 'default'
                "#,
            )
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?,
        };

        sqlx::query(
            r#"
            INSERT INTO nodes (workspace_id, parent_id, name, kind, path_cache)
            VALUES ($1, NULL, '/', 'folder', '/')
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(workspace_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(workspace_id)
    }

    async fn root_for_workspace(&self, workspace_id: Uuid) -> VaultResult<Node> {
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
              AND n.parent_id IS NULL
              AND n.deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .fetch_optional(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        row.map(NodeRow::into_node)
            .ok_or_else(|| VaultRepoError::NotFound("root node not found".into()))
    }
}
