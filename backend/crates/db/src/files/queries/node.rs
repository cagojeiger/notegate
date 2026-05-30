use uuid::Uuid;

use super::super::FilesRepo;
use super::super::error::map_sqlx_error;
use super::super::rows::NodeRow;
use notegate_domain::files::{Children, FilesError, FilesResult, Node, NodeKind};

impl FilesRepo {
    pub(in crate::files) async fn resolve_node(
        &self,
        user_id: Uuid,
        path: String,
    ) -> FilesResult<Node> {
        let workspace_id = self.default_workspace_id(user_id).await?;
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
              AND n.path_cache = $2
              AND n.deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(path)
        .fetch_optional(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        row.map(NodeRow::into_node)
            .ok_or_else(|| FilesError::NotFound("node not found".into()))
    }

    pub(in crate::files) async fn child_nodes(
        &self,
        user_id: Uuid,
        parent_node_id: Uuid,
    ) -> FilesResult<Children> {
        let workspace_id = self.default_workspace_id(user_id).await?;
        let parent = self.node_by_id(workspace_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(FilesError::InvalidInput("node is not a folder".into()));
        }

        let rows = sqlx::query_as::<_, NodeRow>(
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
              AND n.parent_id = $2
              AND n.deleted_at IS NULL
            ORDER BY n.sort_order, n.name
            "#,
        )
        .bind(workspace_id)
        .bind(parent_node_id)
        .fetch_all(self.pool())
        .await
        .map_err(map_sqlx_error)?
        .into_iter()
        .map(NodeRow::into_node)
        .collect();

        Ok(Children {
            parent,
            children: rows,
        })
    }

    pub(super) async fn node_by_id(&self, workspace_id: Uuid, node_id: Uuid) -> FilesResult<Node> {
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
              AND n.id = $2
              AND n.deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .fetch_optional(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        row.map(NodeRow::into_node)
            .ok_or_else(|| FilesError::NotFound("node not found".into()))
    }
}
