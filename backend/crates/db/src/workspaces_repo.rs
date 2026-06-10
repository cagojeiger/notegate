//! Workspace lifecycle persistence.
//!
//! Creating a workspace inserts the `workspaces` row (whose trigger materializes
//! the canonical root node with attribution = the creator) and an explicit
//! `workspace_access(role='owner')` row for the creator. Runtime permissions are
//! resolved from live access rows only.

use crate::{map_sqlx_error, workspace_role};
use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use notegate_model::{CreateWorkspace, WorkspaceCursor, WorkspaceView};
use notegate_model::{Role, Workspace};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct WorkspaceRepo {
    pool: PgPool,
}

impl WorkspaceRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// A row from `workspaces`.
#[derive(Debug, FromRow)]
struct WorkspaceRow {
    id: Uuid,
    name: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by: Option<Uuid>,
    purge_after: Option<DateTime<Utc>>,
}

impl From<WorkspaceRow> for Workspace {
    fn from(row: WorkspaceRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deleted_at: row.deleted_at,
            deleted_by: row.deleted_by,
            purge_after: row.purge_after,
        }
    }
}

#[derive(Debug, FromRow)]
struct WorkspaceViewRow {
    id: Uuid,
    name: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by: Option<Uuid>,
    purge_after: Option<DateTime<Utc>>,
    role: String,
    root_node_id: Uuid,
}

impl WorkspaceViewRow {
    fn into_view(self) -> Result<WorkspaceView> {
        let role = Role::parse(&self.role)
            .ok_or_else(|| Error::internal(format!("unknown workspace role: {}", self.role)))?;
        Ok(WorkspaceView {
            workspace: Workspace {
                id: self.id,
                name: self.name,
                created_by: self.created_by,
                created_at: self.created_at,
                updated_at: self.updated_at,
                deleted_at: self.deleted_at,
                deleted_by: self.deleted_by,
                purge_after: self.purge_after,
            },
            role,
            root_node_id: self.root_node_id,
        })
    }
}

const WORKSPACE_COLUMNS: &str =
    "id, name, created_by, created_at, updated_at, deleted_at, deleted_by, purge_after";

const WORKSPACE_VIEW_SELECT: &str = "SELECT w.id, w.name, w.created_by, w.created_at, w.updated_at, \
                                  w.deleted_at, w.deleted_by, w.purge_after, \
                                  wa.role AS role, \
                                  root.id AS root_node_id \
                           FROM workspaces w \
                           JOIN workspace_access wa ON wa.workspace_id = w.id \
                                                   AND wa.account_id = $1 \
                                                   AND wa.revoked_at IS NULL \
                           JOIN accounts caller ON caller.id = wa.account_id \
                                                AND caller.is_active = true \
                                                AND caller.deleted_at IS NULL \
                                                AND (wa.role <> 'owner' OR caller.kind = 'user') \
                           JOIN nodes root ON root.workspace_id = w.id \
                                          AND root.parent_id IS NULL \
                                          AND root.deleted_at IS NULL";

impl WorkspaceRepo {
    pub async fn role_for(&self, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
        workspace_role::live_role(&self.pool, workspace_id, account_id).await
    }

    pub async fn create_workspace(
        &self,
        creator_account_id: Uuid,
        command: &CreateWorkspace,
    ) -> Result<Workspace> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let owner_exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM accounts \
             WHERE id = $1 AND kind = 'user' AND is_active = true AND deleted_at IS NULL \
             FOR UPDATE",
        )
        .bind(creator_account_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if owner_exists.is_none() {
            return Err(Error::not_found("workspace owner user account not found"));
        }

        // Enforce the creator quota inside the transaction so a concurrent create
        // cannot slip past the cap. Soft-deleted workspaces do not count.
        let owned: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workspaces WHERE created_by = $1 AND deleted_at IS NULL",
        )
        .bind(creator_account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let owned =
            usize::try_from(owned).map_err(|_error| Error::internal("negative workspace count"))?;
        if owned >= limits::OWNED_WORKSPACES_MAX {
            return Err(Error::conflict(format!(
                "owner already has the maximum of {} workspaces",
                limits::OWNED_WORKSPACES_MAX
            )));
        }

        let accessible: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workspace_access wa \
             JOIN workspaces w ON w.id = wa.workspace_id AND w.deleted_at IS NULL \
             WHERE wa.account_id = $1 AND wa.revoked_at IS NULL",
        )
        .bind(creator_account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let accessible = usize::try_from(accessible)
            .map_err(|_error| Error::internal("negative accessible workspace count"))?;
        if accessible >= limits::ACCESSIBLE_WORKSPACES_PER_ACCOUNT_MAX {
            return Err(Error::conflict(format!(
                "account already has the maximum of {} accessible workspaces",
                limits::ACCESSIBLE_WORKSPACES_PER_ACCOUNT_MAX
            )));
        }

        // Insert the workspace; the AFTER INSERT trigger creates the root node
        // with created_by = updated_by = this workspace's created_by.
        let row = sqlx::query_as::<_, WorkspaceRow>(&format!(
            "INSERT INTO workspaces (name, created_by) \
             VALUES ($1, $2) RETURNING {WORKSPACE_COLUMNS}"
        ))
        .bind(&command.name)
        .bind(creator_account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        sqlx::query(
            "INSERT INTO workspace_access (workspace_id, account_id, role, granted_by) \
             VALUES ($1, $2, 'owner', $2)",
        )
        .bind(row.id)
        .bind(creator_account_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(Workspace::from(row))
    }

    pub async fn find_workspace(&self, workspace_id: Uuid) -> Result<Option<Workspace>> {
        let row = sqlx::query_as::<_, WorkspaceRow>(&format!(
            "SELECT {WORKSPACE_COLUMNS} FROM workspaces WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(Workspace::from))
    }

    pub async fn find_workspace_view_for(
        &self,
        account_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<WorkspaceView>> {
        let row = sqlx::query_as::<_, WorkspaceViewRow>(&format!(
            "{WORKSPACE_VIEW_SELECT} \
             WHERE w.id = $2 AND w.deleted_at IS NULL"
        ))
        .bind(account_id)
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.map(WorkspaceViewRow::into_view).transpose()
    }

    pub async fn list_workspace_views_by_name_for(
        &self,
        account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> Result<Vec<WorkspaceView>> {
        let rows = sqlx::query_as::<_, WorkspaceViewRow>(&format!(
            "{WORKSPACE_VIEW_SELECT} \
             WHERE w.deleted_at IS NULL AND w.name = $2 \
             ORDER BY w.created_at, w.id LIMIT $3"
        ))
        .bind(account_id)
        .bind(name)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.into_iter().map(WorkspaceViewRow::into_view).collect()
    }

    pub async fn list_workspace_views_for(
        &self,
        account_id: Uuid,
        limit: i64,
        cursor: Option<&WorkspaceCursor>,
    ) -> Result<Vec<WorkspaceView>> {
        let rows = match cursor {
            None => {
                sqlx::query_as::<_, WorkspaceViewRow>(&format!(
                    "{WORKSPACE_VIEW_SELECT} \
                     WHERE w.deleted_at IS NULL \
                     ORDER BY w.created_at, w.id LIMIT $2"
                ))
                .bind(account_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
            Some(cursor) => {
                sqlx::query_as::<_, WorkspaceViewRow>(&format!(
                    "{WORKSPACE_VIEW_SELECT} \
                     WHERE w.deleted_at IS NULL \
                       AND (w.created_at, w.id) > ($2, $3) \
                     ORDER BY w.created_at, w.id LIMIT $4"
                ))
                .bind(account_id)
                .bind(cursor.created_at)
                .bind(cursor.id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(map_sqlx_error)?;

        rows.into_iter().map(WorkspaceViewRow::into_view).collect()
    }

    pub async fn rename_workspace(&self, workspace_id: Uuid, new_name: &str) -> Result<Workspace> {
        let row = sqlx::query_as::<_, WorkspaceRow>(&format!(
            "UPDATE workspaces SET name = $2, updated_at = now() \
             WHERE id = $1 AND deleted_at IS NULL RETURNING {WORKSPACE_COLUMNS}"
        ))
        .bind(workspace_id)
        .bind(new_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_constraint_error)?
        .ok_or_else(|| Error::not_found("workspace not found"))?;
        Ok(Workspace::from(row))
    }

    pub async fn delete_workspace(&self, workspace_id: Uuid, deleted_by: Uuid) -> Result<()> {
        let result = sqlx::query(
            "UPDATE workspaces \
             SET deleted_at = now(), deleted_by = $2, \
                 purge_after = now() + make_interval(days => $3::int), updated_at = now() \
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(deleted_by)
        .bind(i32::try_from(limits::DELETED_NODE_RETENTION_DAYS).unwrap_or(i32::MAX))
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("workspace not found"));
        }
        Ok(())
    }

    pub async fn root_node_id(&self, workspace_id: Uuid) -> Result<Option<Uuid>> {
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT root.id FROM nodes root \
             JOIN workspaces w ON w.id = root.workspace_id \
             WHERE root.workspace_id = $1 AND root.parent_id IS NULL \
               AND root.deleted_at IS NULL AND w.deleted_at IS NULL",
        )
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(id)
    }
}

/// Map a unique/check violation to a clean validation error, falling back to the
/// generic internal mapping for everything else. Used on workspace insert/rename
/// so a `(created_by, name)` conflict or bad name surfaces as 4xx.
fn map_constraint_error(error: sqlx::Error) -> Error {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.is_unique_violation() {
            return Error::conflict("a workspace with this name already exists");
        }
        if db_error.is_check_violation() {
            return Error::validation("workspace name is invalid");
        }
    }
    map_sqlx_error(error)
}
