use uuid::Uuid;

use super::super::FilesRepo;
use super::super::error::map_sqlx_error;
use super::{default_workspace_id_tx, live_node_for_update, lock_subtree_tx, lock_workspace_tx};
use notegate_domain::files::{FilesError, FilesResult};

impl FilesRepo {
    pub(in crate::files) async fn delete_node_atomic(
        &self,
        user_id: Uuid,
        node_id: Uuid,
    ) -> FilesResult<()> {
        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;
        let workspace_id = default_workspace_id_tx(&mut tx, user_id).await?;
        lock_workspace_tx(&mut tx, workspace_id).await?;

        let node = live_node_for_update(&mut tx, workspace_id, node_id).await?;
        if node.parent_id.is_none() {
            return Err(FilesError::Conflict("root cannot be deleted".into()));
        }
        lock_subtree_tx(&mut tx, workspace_id, node_id).await?;

        let result = sqlx::query(
            r#"
            WITH RECURSIVE descendants AS (
                SELECT id
                FROM nodes
                WHERE workspace_id = $1
                  AND id = $2
                  AND deleted_at IS NULL

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
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(FilesError::NotFound("node not found".into()));
        }

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }
}
