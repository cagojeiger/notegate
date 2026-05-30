mod document;
mod node;
mod search;

use uuid::Uuid;

use super::FilesRepo;
use super::error::map_sqlx_error;
use notegate_domain::files::{FilesError, FilesResult};

impl FilesRepo {
    pub(in crate::files) async fn default_workspace_id(&self, user_id: Uuid) -> FilesResult<Uuid> {
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

        workspace_id.ok_or_else(|| FilesError::NotFound("default workspace not found".into()))
    }
}
