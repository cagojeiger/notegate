//! Workspace lifecycle persistence.
//!
//! Creating a workspace inserts the `workspaces` row (whose trigger materializes
//! the canonical root node with attribution = the creator) and grants the creator
//! `owner` in the same transaction, after enforcing the owner quota in-tx.

use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use notegate_model::{Role, Workspace};
use notegate_service::workspaces::{
    CreateWorkspace, WorkspaceCursor, WorkspaceStore, WorkspaceView,
};
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
    owner_account_id: Uuid,
    name: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<WorkspaceRow> for Workspace {
    fn from(row: WorkspaceRow) -> Self {
        Self {
            id: row.id,
            owner_account_id: row.owner_account_id,
            name: row.name,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct WorkspaceViewRow {
    id: Uuid,
    owner_account_id: Uuid,
    name: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
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
                owner_account_id: self.owner_account_id,
                name: self.name,
                created_by: self.created_by,
                created_at: self.created_at,
                updated_at: self.updated_at,
            },
            role,
            root_node_id: self.root_node_id,
        })
    }
}

const WORKSPACE_COLUMNS: &str = "id, owner_account_id, name, created_by, created_at, updated_at";

const WORKSPACE_VIEW_SELECT: &str = "SELECT w.id, w.owner_account_id, w.name, w.created_by, w.created_at, w.updated_at, \
                                  a.role, root.id AS root_node_id \
                           FROM workspaces w \
                           JOIN workspace_access a ON a.workspace_id = w.id \
                           JOIN accounts acc ON acc.id = a.account_id \
                           JOIN nodes root ON root.workspace_id = w.id \
                                          AND root.parent_id IS NULL \
                                          AND root.deleted_at IS NULL";

impl WorkspaceStore for WorkspaceRepo {
    async fn role_for(&self, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
        live_role(&self.pool, workspace_id, account_id).await
    }

    async fn create_workspace(
        &self,
        owner_account_id: Uuid,
        command: &CreateWorkspace,
    ) -> Result<Workspace> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let owner_exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM accounts \
             WHERE id = $1 AND is_active = true AND deleted_at IS NULL \
             FOR UPDATE",
        )
        .bind(owner_account_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if owner_exists.is_none() {
            return Err(Error::not_found("owner account not found"));
        }

        // Enforce the owner quota inside the transaction so a concurrent create
        // cannot slip past the cap.
        let owned: i64 =
            sqlx::query_scalar("SELECT count(*) FROM workspaces WHERE owner_account_id = $1")
                .bind(owner_account_id)
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

        // Insert the workspace; the AFTER INSERT trigger creates the root node
        // with created_by = updated_by = this workspace's created_by.
        let row = sqlx::query_as::<_, WorkspaceRow>(&format!(
            "INSERT INTO workspaces (owner_account_id, name, created_by) \
             VALUES ($1, $2, $3) RETURNING {WORKSPACE_COLUMNS}"
        ))
        .bind(owner_account_id)
        .bind(&command.name)
        .bind(owner_account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        // Grant the owner `owner` in the same transaction.
        sqlx::query(
            "INSERT INTO workspace_access (workspace_id, account_id, role, granted_by) \
             VALUES ($1, $2, 'owner', $3)",
        )
        .bind(row.id)
        .bind(owner_account_id)
        .bind(owner_account_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(Workspace::from(row))
    }

    async fn find_workspace(&self, workspace_id: Uuid) -> Result<Option<Workspace>> {
        let row = sqlx::query_as::<_, WorkspaceRow>(&format!(
            "SELECT {WORKSPACE_COLUMNS} FROM workspaces WHERE id = $1"
        ))
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(Workspace::from))
    }

    async fn find_workspace_view_for(
        &self,
        account_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<WorkspaceView>> {
        let row = sqlx::query_as::<_, WorkspaceViewRow>(&format!(
            "{WORKSPACE_VIEW_SELECT} \
             WHERE a.account_id = $1 AND a.revoked_at IS NULL \
               AND acc.is_active = true AND acc.deleted_at IS NULL AND w.id = $2"
        ))
        .bind(account_id)
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.map(WorkspaceViewRow::into_view).transpose()
    }

    async fn list_workspace_views_by_name_for(
        &self,
        account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> Result<Vec<WorkspaceView>> {
        let rows = sqlx::query_as::<_, WorkspaceViewRow>(&format!(
            "{WORKSPACE_VIEW_SELECT} \
             WHERE a.account_id = $1 AND a.revoked_at IS NULL \
               AND acc.is_active = true AND acc.deleted_at IS NULL AND w.name = $2 \
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

    async fn list_workspace_views_for(
        &self,
        account_id: Uuid,
        limit: i64,
        cursor: Option<&WorkspaceCursor>,
    ) -> Result<Vec<WorkspaceView>> {
        let rows = match cursor {
            None => {
                sqlx::query_as::<_, WorkspaceViewRow>(&format!(
                    "{WORKSPACE_VIEW_SELECT} \
                     WHERE a.account_id = $1 AND a.revoked_at IS NULL \
                       AND acc.is_active = true AND acc.deleted_at IS NULL \
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
                     WHERE a.account_id = $1 AND a.revoked_at IS NULL \
                       AND acc.is_active = true AND acc.deleted_at IS NULL \
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

    async fn rename_workspace(&self, workspace_id: Uuid, new_name: &str) -> Result<Workspace> {
        let row = sqlx::query_as::<_, WorkspaceRow>(&format!(
            "UPDATE workspaces SET name = $2, updated_at = now() \
             WHERE id = $1 RETURNING {WORKSPACE_COLUMNS}"
        ))
        .bind(workspace_id)
        .bind(new_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_constraint_error)?
        .ok_or_else(|| Error::not_found("workspace not found"))?;
        Ok(Workspace::from(row))
    }

    async fn delete_workspace(&self, workspace_id: Uuid) -> Result<()> {
        // ON DELETE CASCADE removes workspace_access, nodes, and documents.
        sqlx::query("DELETE FROM workspaces WHERE id = $1")
            .bind(workspace_id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        Ok(())
    }

    async fn root_node_id(&self, workspace_id: Uuid) -> Result<Option<Uuid>> {
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM nodes WHERE workspace_id = $1 AND parent_id IS NULL AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(id)
    }
}

/// The caller's live role in a workspace, or `None` if no non-revoked grant
/// from an active account.
async fn live_role(pool: &PgPool, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT a.role FROM workspace_access a \
         JOIN accounts acc ON acc.id = a.account_id \
         WHERE a.workspace_id = $1 AND a.account_id = $2 AND a.revoked_at IS NULL \
           AND acc.is_active = true AND acc.deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    role.map(|value| {
        Role::parse(&value)
            .ok_or_else(|| Error::internal(format!("unknown workspace role: {value}")))
    })
    .transpose()
}

/// Map a unique/check violation to a clean validation error, falling back to the
/// generic internal mapping for everything else. Used on workspace insert/rename
/// so a `(owner_account_id, name)` conflict or bad name surfaces as 4xx.
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

fn map_sqlx_error(error: sqlx::Error) -> Error {
    Error::internal(format!("workspace repository query failed: {error}"))
}
