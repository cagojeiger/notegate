use uuid::Uuid;

use super::error::map_sqlx_error;
use super::rows::{DocumentBundleRow, DocumentRow, NodeRow};
use super::validation::{child_path, validate_document_name};
use super::{DocumentBundle, FilesRepo, FilesRepoError, FilesResult, NodeKind};

impl FilesRepo {
    pub async fn create_document(
        &self,
        user_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> FilesResult<DocumentBundle> {
        validate_document_name(name)?;
        let workspace_id = self.default_workspace_id(user_id).await?;
        let parent = self.node_by_id(workspace_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(FilesRepoError::InvalidInput(
                "parent is not a folder".into(),
            ));
        }

        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;
        let path = child_path(&parent.path, name);
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
        .bind(parent_node_id)
        .bind(name)
        .bind(path)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let document_row = sqlx::query_as::<_, DocumentRow>(
            r#"
            INSERT INTO documents (node_id, workspace_id, content_md, search_text)
            VALUES ($1, $2, '', '')
            RETURNING node_id, workspace_id, content_md, search_text, created_at, updated_at
            "#,
        )
        .bind(node_row.id)
        .bind(workspace_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;

        Ok(DocumentBundle {
            node: node_row.into_node(),
            document: document_row.into_document(),
        })
    }

    pub async fn document(&self, user_id: Uuid, node_id: Uuid) -> FilesResult<DocumentBundle> {
        let workspace_id = self.default_workspace_id(user_id).await?;
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
                d.search_text,
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
        .bind(node_id)
        .fetch_optional(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        row.map(DocumentBundleRow::into_bundle)
            .ok_or_else(|| FilesRepoError::NotFound("document not found".into()))
    }

    pub async fn save_document(
        &self,
        user_id: Uuid,
        node_id: Uuid,
        content_md: &str,
    ) -> FilesResult<DocumentBundle> {
        let workspace_id = self.default_workspace_id(user_id).await?;
        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;

        let exists = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM nodes
                WHERE workspace_id = $1
                  AND id = $2
                  AND kind = 'document'
                  AND deleted_at IS NULL
            )
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        if !exists {
            return Err(FilesRepoError::NotFound("document not found".into()));
        }

        sqlx::query(
            r#"
            UPDATE documents
            SET content_md = $3,
                search_text = $3,
                updated_at = now()
            WHERE workspace_id = $1
              AND node_id = $2
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .bind(content_md)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            r#"
            UPDATE nodes
            SET updated_at = now()
            WHERE workspace_id = $1
              AND id = $2
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;

        self.document(user_id, node_id).await
    }
}
