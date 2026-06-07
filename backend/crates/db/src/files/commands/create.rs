use uuid::Uuid;

use super::super::FilesRepo;
use super::super::error::map_sqlx_error;
use super::super::rows::{DocumentBundleRow, NodeRow};
use super::{
    child_path, default_workspace_id_tx, ensure_folder, live_node_for_update, lock_workspace_tx,
};
use notegate_domain::files::{CreateDocument, CreateFolder, DocumentBundle, FilesResult, Node};

impl FilesRepo {
    pub(in crate::files) async fn create_folder_atomic(
        &self,
        user_id: Uuid,
        command: CreateFolder,
    ) -> FilesResult<Node> {
        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;
        let workspace_id = default_workspace_id_tx(&mut tx, user_id).await?;
        lock_workspace_tx(&mut tx, workspace_id).await?;

        let parent = live_node_for_update(&mut tx, workspace_id, command.parent_node_id).await?;
        ensure_folder(&parent, "parent is not a folder")?;

        let path = child_path(&parent.path, &command.name);
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
        .bind(command.parent_node_id)
        .bind(command.name)
        .bind(path)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(row.into_node())
    }

    pub(in crate::files) async fn create_document_atomic(
        &self,
        user_id: Uuid,
        command: CreateDocument,
    ) -> FilesResult<DocumentBundle> {
        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;
        let workspace_id = default_workspace_id_tx(&mut tx, user_id).await?;
        lock_workspace_tx(&mut tx, workspace_id).await?;

        let parent = live_node_for_update(&mut tx, workspace_id, command.parent_node_id).await?;
        ensure_folder(&parent, "parent is not a folder")?;

        let path = child_path(&parent.path, &command.name);
        let node_row = sqlx::query_as::<_, NodeRow>(
            r#"
            INSERT INTO nodes (workspace_id, parent_id, name, kind, path_cache)
            VALUES ($1, $2, $3, 'document', $4)
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
        .bind(command.parent_node_id)
        .bind(command.name)
        .bind(path)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            r#"
            INSERT INTO documents (node_id, workspace_id)
            VALUES ($1, $2)
            "#,
        )
        .bind(node_row.id)
        .bind(workspace_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let row = sqlx::query_as::<_, DocumentBundleRow>(
            r#"
            SELECT
                n.id,
                n.parent_id,
                n.name,
                n.kind,
                n.path_cache,
                n.sort_order,
                false AS has_children,
                n.created_at AS node_created_at,
                n.updated_at AS node_updated_at,
                d.node_id,
                d.workspace_id,
                d.content_md,
                d.content_sha256,
                d.byte_len,
                d.line_count,
                d.created_at AS document_created_at,
                d.updated_at AS document_updated_at
            FROM nodes n
            JOIN documents d
              ON d.node_id = n.id
             AND d.workspace_id = n.workspace_id
            WHERE n.workspace_id = $1
              AND n.id = $2
              AND n.kind = 'document'
              AND n.deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(node_row.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(row.into_bundle())
    }
}
