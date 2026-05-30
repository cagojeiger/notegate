use uuid::Uuid;

use super::FilesRepo;
use super::error::map_sqlx_error;
use super::rows::{DocumentBundleRow, DocumentRow, NodeRow};
use notegate_domain::files::{DocumentBundle, FilesError, FilesResult};

impl FilesRepo {
    pub(super) async fn create_document_node(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
        path: &str,
    ) -> FilesResult<DocumentBundle> {
        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;
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

    pub(super) async fn document_by_node_id(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> FilesResult<DocumentBundle> {
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
            .ok_or_else(|| FilesError::NotFound("document not found".into()))
    }

    pub(super) async fn save_document_content(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        content_md: &str,
    ) -> FilesResult<()> {
        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;

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
        Ok(())
    }
}
