use uuid::Uuid;

use super::super::FilesRepo;
use super::super::error::map_sqlx_error;
use super::super::rows::DocumentBundleRow;
use notegate_domain::files::{DocumentBundle, FilesError, FilesResult};

impl FilesRepo {
    pub(in crate::files) async fn document_by_node_id(
        &self,
        user_id: Uuid,
        node_id: Uuid,
    ) -> FilesResult<DocumentBundle> {
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
        .bind(node_id)
        .fetch_optional(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        row.map(DocumentBundleRow::into_bundle)
            .ok_or_else(|| FilesError::NotFound("document not found".into()))
    }
}
