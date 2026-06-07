use uuid::Uuid;

use super::super::FilesRepo;
use super::super::error::map_sqlx_error;
use super::super::rows::NodeRow;
use notegate_domain::files::{
    Children, ChildrenCursor, ChildrenPage, ChildrenRequest, FilesError, FilesResult, Node,
    NodeKind, Page,
};

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

    pub(in crate::files) async fn paged_child_nodes(
        &self,
        user_id: Uuid,
        parent_node_id: Uuid,
        request: ChildrenRequest,
    ) -> FilesResult<ChildrenPage> {
        let workspace_id = self.default_workspace_id(user_id).await?;
        let parent = self.node_by_id(workspace_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(FilesError::InvalidInput("node is not a folder".into()));
        }

        let limit = request.limit.unwrap_or(100).clamp(1, 500);
        let fetch_limit = limit + 1;
        let cursor_sort_order = request.cursor.as_ref().map(|cursor| cursor.sort_order);
        let cursor_name = request.cursor.as_ref().map(|cursor| cursor.name.clone());
        let cursor_id = request.cursor.as_ref().map(|cursor| cursor.id);

        let mut rows = sqlx::query_as::<_, NodeRow>(
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
              AND (
                  $3::INTEGER IS NULL
                  OR (n.sort_order, n.name, n.id) > ($3, $4, $5)
              )
            ORDER BY n.sort_order, n.name, n.id
            LIMIT $6
            "#,
        )
        .bind(workspace_id)
        .bind(parent_node_id)
        .bind(cursor_sort_order)
        .bind(cursor_name)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(self.pool())
        .await
        .map_err(map_sqlx_error)?;

        let has_more = rows.len() as i64 > limit;
        if has_more {
            rows.truncate(limit as usize);
        }
        let children = rows.into_iter().map(NodeRow::into_node).collect::<Vec<_>>();
        let next_cursor = if has_more {
            children.last().map(|node| ChildrenCursor {
                sort_order: node.sort_order,
                name: node.name.clone(),
                id: node.id,
            })
        } else {
            None
        };

        Ok(ChildrenPage {
            parent,
            page: Page {
                items: children,
                limit,
                has_more,
                next_cursor,
            },
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
