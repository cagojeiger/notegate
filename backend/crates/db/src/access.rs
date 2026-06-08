//! Workspace access persistence.
//!
//! This adapter implements access-list management separately from workspace
//! lifecycle operations so RBAC changes remain isolated from workspace creation,
//! rename, and delete queries.

use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use notegate_model::account::AccountKind;
use notegate_model::{Role, WorkspaceAccess};
use notegate_service::access::{AccessStore, GrantAccess};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AccessRepo {
    pool: PgPool,
}

impl AccessRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// A row from `workspace_access`.
#[derive(Debug, FromRow)]
struct WorkspaceAccessRow {
    workspace_id: Uuid,
    account_id: Uuid,
    role: String,
    granted_by: Option<Uuid>,
    granted_at: DateTime<Utc>,
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
            granted_by: self.granted_by,
            granted_at: self.granted_at,
            revoked_at: self.revoked_at,
            revoked_by: self.revoked_by,
        })
    }
}

const ACCESS_RETURNING_COLUMNS: &str =
    "workspace_id, account_id, role, granted_by, granted_at, revoked_at, revoked_by";
const ACCESS_SELECT_COLUMNS: &str = "wa.workspace_id, wa.account_id, wa.role, wa.granted_by, wa.granted_at, wa.revoked_at, wa.revoked_by";

impl AccessStore for AccessRepo {
    async fn role_for(&self, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
        live_role(&self.pool, workspace_id, account_id).await
    }

    async fn list_access(&self, workspace_id: Uuid) -> Result<Vec<WorkspaceAccess>> {
        let rows = sqlx::query_as::<_, WorkspaceAccessRow>(&format!(
            "SELECT {ACCESS_SELECT_COLUMNS} FROM workspace_access wa \
             JOIN accounts acc ON acc.id = wa.account_id \
             WHERE wa.workspace_id = $1 AND wa.revoked_at IS NULL \
               AND acc.is_active = true AND acc.deleted_at IS NULL \
             ORDER BY wa.granted_at, wa.account_id"
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
        granted_by: Uuid,
    ) -> Result<WorkspaceAccess> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        lock_workspace(&mut tx, command.workspace_id).await?;
        ensure_account_can_receive_role(&mut tx, command.account_id, command.role).await?;

        guard_last_owner(
            &mut tx,
            command.workspace_id,
            command.account_id,
            Some(command.role),
        )
        .await?;

        // Count live accounts other than the target so re-granting an existing
        // account never trips the cap, but activating a new (or revoked) account
        // respects [`limits::WORKSPACE_ACCESS_MAX_ACCOUNTS`]. Inactive/deleted
        // accounts are not live access accounts.
        let active_others: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workspace_access wa \
             JOIN accounts acc ON acc.id = wa.account_id \
             WHERE wa.workspace_id = $1 AND wa.account_id <> $2 AND wa.revoked_at IS NULL \
               AND acc.is_active = true AND acc.deleted_at IS NULL",
        )
        .bind(command.workspace_id)
        .bind(command.account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let active_others = usize::try_from(active_others)
            .map_err(|_error| Error::internal("negative access count"))?;
        if active_others >= limits::WORKSPACE_ACCESS_MAX_ACCOUNTS {
            return Err(Error::conflict(format!(
                "workspace already has the maximum of {} active access accounts",
                limits::WORKSPACE_ACCESS_MAX_ACCOUNTS
            )));
        }

        // PK is (workspace_id, account_id): re-granting (including reviving a
        // revoked row) updates in place and clears the revocation.
        let row = sqlx::query_as::<_, WorkspaceAccessRow>(&format!(
            "INSERT INTO workspace_access (workspace_id, account_id, role, granted_by) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (workspace_id, account_id) DO UPDATE \
             SET role = EXCLUDED.role, granted_by = EXCLUDED.granted_by, \
                 granted_at = now(), revoked_at = NULL, revoked_by = NULL \
             RETURNING {ACCESS_RETURNING_COLUMNS}"
        ))
        .bind(command.workspace_id)
        .bind(command.account_id)
        .bind(command.role.as_str())
        .bind(granted_by)
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
        lock_workspace(&mut tx, workspace_id).await?;
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

/// The caller's live role in a workspace, or `None` if no non-revoked grant
/// from an active account.
async fn live_role(pool: &PgPool, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT wa.role FROM workspace_access wa \
         JOIN accounts acc ON acc.id = wa.account_id \
         WHERE wa.workspace_id = $1 AND wa.account_id = $2 AND wa.revoked_at IS NULL \
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

async fn lock_workspace(tx: &mut sqlx::PgConnection, workspace_id: Uuid) -> Result<()> {
    let found: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE id = $1 FOR UPDATE")
            .bind(workspace_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
    if found.is_none() {
        return Err(Error::not_found("workspace not found"));
    }
    Ok(())
}

async fn ensure_account_can_receive_role(
    tx: &mut sqlx::PgConnection,
    account_id: Uuid,
    role: Role,
) -> Result<()> {
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT kind FROM accounts \
         WHERE id = $1 AND is_active = true AND deleted_at IS NULL",
    )
    .bind(account_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let kind = kind.ok_or_else(|| Error::not_found("account not found"))?;
    let kind = AccountKind::parse(&kind)
        .ok_or_else(|| Error::internal(format!("unknown account kind: {kind}")))?;
    if kind == AccountKind::Agent && role == Role::Owner {
        return Err(Error::validation(
            "agent accounts cannot receive workspace owner role",
        ));
    }
    Ok(())
}

/// Lock the live owner rows and reject a change that would leave the workspace
/// with no active owner. The service pre-checks this for a clean conflict
/// response; the transaction guard keeps concurrent owner changes from racing
/// through it.
async fn guard_last_owner(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    account_id: Uuid,
    next_role: Option<Role>,
) -> Result<()> {
    let owners: Vec<Uuid> = sqlx::query_scalar(
        "SELECT wa.account_id FROM workspace_access wa \
         JOIN accounts acc ON acc.id = wa.account_id \
         WHERE wa.workspace_id = $1 AND wa.role = 'owner' AND wa.revoked_at IS NULL \
           AND acc.is_active = true AND acc.deleted_at IS NULL \
         FOR UPDATE OF wa",
    )
    .bind(workspace_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let target_is_owner = owners.contains(&account_id);
    let target_remains_owner = next_role == Some(Role::Owner);
    if target_is_owner && !target_remains_owner && owners.len() <= 1 {
        return Err(Error::conflict("workspace must retain at least one owner"));
    }

    Ok(())
}

fn map_sqlx_error(error: sqlx::Error) -> Error {
    Error::internal(format!("access repository query failed: {error}"))
}
