//! Workspace access persistence.
//!
//! Access rows store explicit owner/editor/viewer workspace membership. Runtime
//! permissions are resolved from live access rows only.

use crate::{map_sqlx_error, workspace_role};
use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use notegate_model::GrantAccess;
use notegate_model::{Role, WorkspaceAccess};
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

impl AccessRepo {
    pub async fn role_for(&self, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
        workspace_role::live_role(&self.pool, workspace_id, account_id).await
    }

    pub async fn list_access(&self, workspace_id: Uuid) -> Result<Vec<WorkspaceAccess>> {
        let rows = sqlx::query_as::<_, WorkspaceAccessRow>(&format!(
            "SELECT {ACCESS_SELECT_COLUMNS} FROM workspace_access wa \
             JOIN workspaces w ON w.id = wa.workspace_id AND w.deleted_at IS NULL \
             JOIN accounts acc ON acc.id = wa.account_id \
             WHERE wa.workspace_id = $1 AND wa.revoked_at IS NULL \
               AND acc.is_active = true AND acc.deleted_at IS NULL \
               AND (wa.role <> 'owner' OR acc.kind = 'user') \
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

    pub async fn upsert_access(
        &self,
        command: &GrantAccess,
        granted_by: Uuid,
    ) -> Result<WorkspaceAccess> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let workspace_creator = lock_workspace(&mut tx, command.workspace_id).await?;
        ensure_account_can_receive_role(&mut tx, command.account_id, command.role).await?;
        protect_owner_change(
            &mut tx,
            command.workspace_id,
            command.account_id,
            workspace_creator,
            command.role,
        )
        .await?;

        // Count live grants other than the target so re-granting an existing
        // account never trips the cap, but activating a new (or revoked) account
        // respects [`limits::WORKSPACE_ACCESS_MAX_ACCOUNTS`]. The count uses
        // the same effective-live predicate as `list_access`/`role_for`.
        let active_others: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workspace_access wa \
             JOIN accounts acc ON acc.id = wa.account_id \
             WHERE wa.workspace_id = $1 AND wa.account_id <> $2 AND wa.revoked_at IS NULL \
               AND acc.is_active = true AND acc.deleted_at IS NULL \
               AND (wa.role <> 'owner' OR acc.kind = 'user')",
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

        // Count live workspace grants for the target account other than this
        // workspace, so updating/reviving the same workspace row does not trip
        // the per-account accessible-workspace cap.
        let accessible_others: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workspace_access wa \
             JOIN workspaces w ON w.id = wa.workspace_id AND w.deleted_at IS NULL \
             JOIN accounts acc ON acc.id = wa.account_id \
             WHERE wa.account_id = $1 AND wa.workspace_id <> $2 AND wa.revoked_at IS NULL \
               AND acc.is_active = true AND acc.deleted_at IS NULL \
               AND (wa.role <> 'owner' OR acc.kind = 'user')",
        )
        .bind(command.account_id)
        .bind(command.workspace_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let accessible_others = usize::try_from(accessible_others)
            .map_err(|_error| Error::internal("negative accessible workspace count"))?;
        if accessible_others >= limits::ACCESSIBLE_WORKSPACES_PER_ACCOUNT_MAX {
            return Err(Error::conflict(format!(
                "account already has the maximum of {} accessible workspaces",
                limits::ACCESSIBLE_WORKSPACES_PER_ACCOUNT_MAX
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

    pub async fn revoke_access(
        &self,
        workspace_id: Uuid,
        account_id: Uuid,
        revoked_by: Uuid,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let workspace_creator = lock_workspace(&mut tx, workspace_id).await?;
        protect_owner_revoke(&mut tx, workspace_id, account_id, workspace_creator).await?;

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

async fn lock_workspace(tx: &mut sqlx::PgConnection, workspace_id: Uuid) -> Result<Uuid> {
    let created_by: Option<Uuid> = sqlx::query_scalar(
        "SELECT created_by FROM workspaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(workspace_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    created_by.ok_or_else(|| Error::not_found("workspace not found"))
}

async fn ensure_account_can_receive_role(
    tx: &mut sqlx::PgConnection,
    account_id: Uuid,
    role: Role,
) -> Result<()> {
    // Check existence/activeness FIRST, independent of the requested role, so an
    // unusable target account yields the same `404 not_found` for every role
    // instead of leaking the difference as owner=409 vs editor/viewer=404.
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT kind FROM accounts \
         WHERE id = $1 AND is_active = true AND deleted_at IS NULL",
    )
    .bind(account_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let Some(kind) = kind else {
        return Err(Error::not_found("account not found"));
    };

    // The account is valid; only now is an owner/kind mismatch a real conflict.
    if role == Role::Owner && kind != "user" {
        return Err(Error::conflict("owner role requires a user account"));
    }
    Ok(())
}

async fn protect_owner_change(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    account_id: Uuid,
    workspace_creator: Uuid,
    new_role: Role,
) -> Result<()> {
    if new_role == Role::Owner {
        return Ok(());
    }
    let Some(current_role) = live_role_for_update(tx, workspace_id, account_id).await? else {
        return Ok(());
    };
    if current_role != Role::Owner {
        return Ok(());
    }
    if account_id == workspace_creator {
        return Err(Error::conflict(
            "workspace creator owner access cannot be downgraded",
        ));
    }
    ensure_another_active_user_owner(tx, workspace_id, account_id).await
}

async fn protect_owner_revoke(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    account_id: Uuid,
    workspace_creator: Uuid,
) -> Result<()> {
    let Some(current_role) = live_role_for_update(tx, workspace_id, account_id).await? else {
        return Ok(());
    };
    if current_role != Role::Owner {
        return Ok(());
    }
    if account_id == workspace_creator {
        return Err(Error::conflict(
            "workspace creator owner access cannot be revoked",
        ));
    }
    ensure_another_active_user_owner(tx, workspace_id, account_id).await
}

async fn live_role_for_update(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    account_id: Uuid,
) -> Result<Option<Role>> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT wa.role FROM workspace_access wa \
         JOIN accounts acc ON acc.id = wa.account_id \
                          AND acc.is_active = true \
                          AND acc.deleted_at IS NULL \
         WHERE wa.workspace_id = $1 AND wa.account_id = $2 AND wa.revoked_at IS NULL \
           AND (wa.role <> 'owner' OR acc.kind = 'user') \
         FOR UPDATE OF wa",
    )
    .bind(workspace_id)
    .bind(account_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    role.map(|value| {
        Role::parse(&value)
            .ok_or_else(|| Error::internal(format!("unknown workspace role: {value}")))
    })
    .transpose()
}

async fn ensure_another_active_user_owner(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    account_id: Uuid,
) -> Result<()> {
    let owners: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_access wa \
         JOIN accounts acc ON acc.id = wa.account_id \
         WHERE wa.workspace_id = $1 AND wa.account_id <> $2 \
           AND wa.role = 'owner' AND wa.revoked_at IS NULL \
           AND acc.kind = 'user' AND acc.is_active = true AND acc.deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if owners <= 0 {
        return Err(Error::conflict(
            "workspace must keep at least one active user owner",
        ));
    }
    Ok(())
}
