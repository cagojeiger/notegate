use uuid::Uuid;

use super::super::FilesRepo;
use super::super::error::map_sqlx_error;
use super::super::rows::DocumentBundleRow;
use super::default_workspace_id_tx;
use notegate_domain::files::{DocumentBundle, FilesError, FilesResult, SaveDocument};

impl FilesRepo {
    pub(in crate::files) async fn save_document_atomic(
        &self,
        user_id: Uuid,
        command: SaveDocument,
    ) -> FilesResult<DocumentBundle> {
        let mut tx = self.pool().begin().await.map_err(map_sqlx_error)?;
        let workspace_id = default_workspace_id_tx(&mut tx, user_id).await?;

        select_document_bundle_for_update(&mut tx, workspace_id, command.node_id)
            .await?
            .ok_or_else(|| FilesError::NotFound("document not found".into()))?;

        let document_result = sqlx::query(
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
        .bind(command.node_id)
        .bind(command.content_md)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if document_result.rows_affected() != 1 {
            return Err(FilesError::NotFound("document not found".into()));
        }

        let node_result = sqlx::query(
            r#"
            UPDATE nodes
            SET updated_at = now()
            WHERE workspace_id = $1
              AND id = $2
              AND deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(command.node_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if node_result.rows_affected() != 1 {
            return Err(FilesError::NotFound("document not found".into()));
        }

        let saved = select_document_bundle_for_update(&mut tx, workspace_id, command.node_id)
            .await?
            .ok_or_else(|| FilesError::NotFound("document not found".into()))?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(saved.into_bundle())
    }
}

pub(super) async fn select_document_bundle_for_update(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace_id: Uuid,
    node_id: Uuid,
) -> FilesResult<Option<DocumentBundleRow>> {
    sqlx::query_as::<_, DocumentBundleRow>(
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
        FOR UPDATE OF n, d
        "#,
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx_error)
}
