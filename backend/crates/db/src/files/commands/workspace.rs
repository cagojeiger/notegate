use uuid::Uuid;

use super::super::FilesRepo;
use super::super::error::map_sqlx_error;
use super::super::rows::NodeRow;
use notegate_domain::files::{FilesError, FilesResult, Node};

impl FilesRepo {
    pub(in crate::files) async fn initialize_root_node(&self, user_id: Uuid) -> FilesResult<Node> {
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
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;

        row.map(NodeRow::into_node)
            .ok_or_else(|| FilesError::NotFound("root node not found".into()))
    }
}
