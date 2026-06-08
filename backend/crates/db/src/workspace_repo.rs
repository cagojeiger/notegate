//! Workspaces + workspace_access persistence.
//!
//! All queries use runtime-checked `query_as::<_, Row>()` / `query()` — never
//! the `query!` macro — so a schema reset never breaks compilation. Creating a
//! workspace inserts the `workspaces` row (whose trigger materializes the
//! canonical root node with attribution = the creator) and grants the creator
//! `owner` in the same transaction, after enforcing the owner quota in-tx.
//!
//! [`WorkspaceRepo::role_for`] reads the caller's live (`revoked_at IS NULL`)
//! role; `None` means the caller has no access and every later feature treats
//! that as a 404.

use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use notegate_model::{Role, Workspace, WorkspaceAccess};
use notegate_service::access::{AccessStore, GrantAccess};
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

/// A row from `workspace_access`.
#[derive(Debug, FromRow)]
struct WorkspaceAccessRow {
    workspace_id: Uuid,
    account_id: Uuid,
    role: String,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    revoked_by: Option<Uuid>,
}

impl WorkspaceAccessRow {
    fn into_access(self) -> Result<WorkspaceAccess> {
        let role = Role::parse(&self.role)
            .ok_or_else(|| Error::internal(format!("unknown workspace role: {}", self.role)))?;
        Ok(WorkspaceAccess {
            workspace_id: self.workspace_id,
            account_id: self.account_id,
            role,
            created_by: self.created_by,
            created_at: self.created_at,
            revoked_at: self.revoked_at,
            revoked_by: self.revoked_by,
        })
    }
}

const WORKSPACE_COLUMNS: &str = "id, owner_account_id, name, created_by, created_at, updated_at";
const ACCESS_COLUMNS: &str =
    "workspace_id, account_id, role, created_by, created_at, revoked_at, revoked_by";

const WORKSPACE_VIEW_SELECT: &str = "SELECT w.id, w.owner_account_id, w.name, w.created_by, w.created_at, w.updated_at, \
                                  a.role, root.id AS root_node_id \
                           FROM workspaces w \
                           JOIN workspace_access a ON a.workspace_id = w.id \
                           JOIN nodes root ON root.workspace_id = w.id \
                                          AND root.parent_id IS NULL \
                                          AND root.deleted_at IS NULL";

impl WorkspaceRepo {
    /// The caller's live role in a workspace, or `None` if no non-revoked grant.
    /// Shared by both store-trait implementations and the authorization path.
    async fn live_role(&self, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
        let role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM workspace_access \
             WHERE workspace_id = $1 AND account_id = $2 AND revoked_at IS NULL",
        )
        .bind(workspace_id)
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        role.map(|value| {
            Role::parse(&value)
                .ok_or_else(|| Error::internal(format!("unknown workspace role: {value}")))
        })
        .transpose()
    }
}

impl WorkspaceStore for WorkspaceRepo {
    async fn role_for(&self, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
        self.live_role(workspace_id, account_id).await
    }

    async fn create_workspace(
        &self,
        command: &CreateWorkspace,
        created_by: Uuid,
    ) -> Result<Workspace> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        // Enforce the owner quota inside the transaction so a concurrent create
        // cannot slip past the cap.
        let owned: i64 =
            sqlx::query_scalar("SELECT count(*) FROM workspaces WHERE owner_account_id = $1")
                .bind(command.owner_account_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        let owned =
            usize::try_from(owned).map_err(|_error| Error::internal("negative workspace count"))?;
        if owned >= limits::OWNED_WORKSPACES_MAX {
            return Err(Error::validation(format!(
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
        .bind(command.owner_account_id)
        .bind(&command.name)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        // Grant the creator `owner` in the same transaction.
        sqlx::query(
            "INSERT INTO workspace_access (workspace_id, account_id, role, created_by) \
             VALUES ($1, $2, 'owner', $3)",
        )
        .bind(row.id)
        .bind(command.owner_account_id)
        .bind(created_by)
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
             WHERE a.account_id = $1 AND a.revoked_at IS NULL AND w.id = $2"
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
             WHERE a.account_id = $1 AND a.revoked_at IS NULL AND w.name = $2 \
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
            "SELECT id FROM nodes WHERE workspace_id = $1 AND parent_id IS NULL",
        )
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(id)
    }
}

impl AccessStore for WorkspaceRepo {
    async fn role_for(&self, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
        self.live_role(workspace_id, account_id).await
    }

    async fn list_access(&self, workspace_id: Uuid) -> Result<Vec<WorkspaceAccess>> {
        let rows = sqlx::query_as::<_, WorkspaceAccessRow>(&format!(
            "SELECT {ACCESS_COLUMNS} FROM workspace_access \
             WHERE workspace_id = $1 AND revoked_at IS NULL \
             ORDER BY created_at, account_id"
        ))
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.into_iter()
            .map(WorkspaceAccessRow::into_access)
            .collect()
    }

    async fn upsert_access(
        &self,
        command: &GrantAccess,
        created_by: Uuid,
    ) -> Result<WorkspaceAccess> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        guard_last_owner(
            &mut tx,
            command.workspace_id,
            command.account_id,
            Some(command.role),
        )
        .await?;

        // Count active accounts other than the target so re-granting an existing
        // account never trips the cap, but activating a new (or revoked) account
        // respects [`limits::WORKSPACE_ACCESS_MAX_ACCOUNTS`].
        let active_others: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workspace_access \
             WHERE workspace_id = $1 AND account_id <> $2 AND revoked_at IS NULL",
        )
        .bind(command.workspace_id)
        .bind(command.account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let active_others = usize::try_from(active_others)
            .map_err(|_error| Error::internal("negative access count"))?;
        if active_others >= limits::WORKSPACE_ACCESS_MAX_ACCOUNTS {
            return Err(Error::validation(format!(
                "workspace already has the maximum of {} active access accounts",
                limits::WORKSPACE_ACCESS_MAX_ACCOUNTS
            )));
        }

        // PK is (workspace_id, account_id): re-granting (including reviving a
        // revoked row) updates in place and clears the revocation.
        let row = sqlx::query_as::<_, WorkspaceAccessRow>(&format!(
            "INSERT INTO workspace_access (workspace_id, account_id, role, created_by) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (workspace_id, account_id) DO UPDATE \
             SET role = EXCLUDED.role, created_by = EXCLUDED.created_by, \
                 created_at = now(), revoked_at = NULL, revoked_by = NULL \
             RETURNING {ACCESS_COLUMNS}"
        ))
        .bind(command.workspace_id)
        .bind(command.account_id)
        .bind(command.role.as_str())
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        row.into_access()
    }

    async fn revoke_access(
        &self,
        workspace_id: Uuid,
        account_id: Uuid,
        revoked_by: Uuid,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        guard_last_owner(&mut tx, workspace_id, account_id, None).await?;

        sqlx::query(
            "UPDATE workspace_access SET revoked_at = now(), revoked_by = $3 \
             WHERE workspace_id = $1 AND account_id = $2 AND revoked_at IS NULL",
        )
        .bind(workspace_id)
        .bind(account_id)
        .bind(revoked_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }
}

/// Lock the live owner rows and reject a change that would leave the workspace
/// with no owner. The service pre-checks this for a clean conflict response; the
/// transaction guard keeps concurrent owner changes from racing through it.
async fn guard_last_owner(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    account_id: Uuid,
    next_role: Option<Role>,
) -> Result<()> {
    let owners: Vec<Uuid> = sqlx::query_scalar(
        "SELECT account_id FROM workspace_access \
         WHERE workspace_id = $1 AND role = 'owner' AND revoked_at IS NULL \
         FOR UPDATE",
    )
    .bind(workspace_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let target_is_owner = owners.iter().any(|id| *id == account_id);
    let target_remains_owner = next_role == Some(Role::Owner);
    if target_is_owner && !target_remains_owner && owners.len() <= 1 {
        return Err(Error::validation(
            "workspace must retain at least one owner",
        ));
    }

    Ok(())
}

/// Map a unique/check violation to a clean validation error, falling back to the
/// generic internal mapping for everything else. Used on workspace insert/rename
/// so a `(owner_account_id, name)` conflict or bad name surfaces as 4xx.
fn map_constraint_error(error: sqlx::Error) -> Error {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.is_unique_violation() {
            return Error::validation("a workspace with this name already exists");
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
