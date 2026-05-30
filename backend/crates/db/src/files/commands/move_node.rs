use uuid::Uuid;

use super::super::FilesRepo;
use super::super::error::map_sqlx_error;
use super::{
    child_path, default_workspace_id_tx, ensure_folder, live_node_for_update, lock_subtree_tx,
    lock_workspace_tx, node_by_id_tx, validate_final_name,
};
use notegate_domain::files::{FilesError, FilesResult, MoveNode, Node};

impl FilesRepo {
    pub(in crate::files) async fn move_node_atomic(
        &self,
        user_id: Uuid,
        command: MoveNode,
    ) -> FilesResult<Node> {
        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;
        let workspace_id = default_workspace_id_tx(&mut tx, user_id).await?;
        lock_workspace_tx(&mut tx, workspace_id).await?;

        let node = live_node_for_update(&mut tx, workspace_id, command.node_id).await?;
        if node.parent_id.is_none() {
            return Err(FilesError::Conflict("root cannot be moved".into()));
        }

        let new_parent =
            live_node_for_update(&mut tx, workspace_id, command.new_parent_node_id).await?;
        ensure_folder(&new_parent, "new parent is not a folder")?;
        lock_subtree_tx(&mut tx, workspace_id, command.node_id).await?;

        let final_name = command.new_name.as_deref().unwrap_or(&node.name);
        validate_final_name(&node.kind, final_name)?;

        if node.id == new_parent.id
            || new_parent.path == node.path
            || new_parent
                .path
                .starts_with(&format!("{}/", node.path.trim_end_matches('/')))
        {
            return Err(FilesError::Conflict(
                "node cannot move into itself or its descendant".into(),
            ));
        }

        let old_path = node.path.clone();
        let new_path = child_path(&new_parent.path, final_name);
        let node_result = sqlx::query(
            r#"
            UPDATE nodes
            SET parent_id = $3,
                name = $4,
                updated_at = now()
            WHERE workspace_id = $1
              AND id = $2
              AND deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(command.node_id)
        .bind(command.new_parent_node_id)
        .bind(final_name)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if node_result.rows_affected() != 1 {
            return Err(FilesError::NotFound("node not found".into()));
        }

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
        .bind(command.node_id)
        .bind(old_path)
        .bind(new_path)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let moved = node_by_id_tx(&mut tx, workspace_id, command.node_id).await?;
        tx.commit().await.map_err(map_sqlx_error)?;

        Ok(moved)
    }
}
