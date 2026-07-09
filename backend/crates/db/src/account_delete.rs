//! Account-deletion cascade (ADR 0004).
//!
//! Soft-deletes a user account and tears down everything it owns that must not
//! outlive it: agents, API keys, browser sessions, and space-agent
//! connections. Spaces are deliberately untouched here — the caller must
//! delete owned spaces first (enforced by [`crate::account_repo::AccountRepo`]
//! before this cascade runs). Everything below runs in one transaction with a
//! single `account.delete` audit event recording the row counts touched.

use crate::account_repo::SOLE_OWNED_SPACES_FROM_WHERE;
use crate::audit_events::{self, AccountDeleteCounts, AuditContext};
use crate::map_sqlx_error;
use notegate_core::{Error, Result};
use sqlx::PgPool;
use uuid::Uuid;

/// Soft-delete a user account (ADR 0004). Mark it deleted and tear down owned
/// agents, keys, and access, but do not delete spaces. If the user still owns
/// any live space, reject so the caller must delete those spaces first. KEEP
/// `provider_sub_hash` and PII as a
/// tombstone. The purge run anonymizes PII and frees the sub-hash once the
/// retention window elapses; re-login during the window is rejected by
/// `upsert_user_by_sub`, so a returning sub never duplicates the account.
pub(crate) async fn soft_delete_user(
    pool: &PgPool,
    account_id: Uuid,
    deleted_by: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    let locked: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM accounts \
         WHERE id = $1 AND kind = 'user' AND is_active = true AND deleted_at IS NULL \
         FOR UPDATE",
    )
    .bind(account_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if locked.is_none() {
        return Err(Error::not_found("active user account not found"));
    }

    let _locked_owned_spaces: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM spaces \
         WHERE owner_user_id = $1 AND deleted_at IS NULL \
         FOR UPDATE",
    )
    .bind(account_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let sole_owned: i64 =
        sqlx::query_scalar(&format!("SELECT count(*) {SOLE_OWNED_SPACES_FROM_WHERE}"))
            .bind(account_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
    if sole_owned > 0 {
        return Err(Error::conflict(format!(
            "delete your {sole_owned} owned space(s) before deleting your account"
        )));
    }

    let owned_agents: Vec<Uuid> = sqlx::query_scalar(
        "SELECT a.id FROM agents a \
         JOIN accounts acc ON acc.id = a.id \
         WHERE a.owner_user_id = $1 AND acc.is_active = true AND acc.deleted_at IS NULL \
         FOR UPDATE OF acc",
    )
    .bind(account_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let deactivated_agents = sqlx::query(
        "UPDATE accounts \
         SET is_active = false, deleted_at = now(), deleted_by_account_id = $2, updated_at = now() \
         WHERE id = ANY($1) AND kind = 'agent' AND deleted_at IS NULL",
    )
    .bind(&owned_agents)
    .bind(deleted_by)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let revoked_api_keys = sqlx::query(
        "UPDATE api_keys \
         SET revoked_at = now(), revoked_by_user_id = $2 \
         WHERE revoked_at IS NULL \
           AND (account_id = $1 OR account_id = ANY($3))",
    )
    .bind(account_id)
    .bind(deleted_by)
    .bind(&owned_agents)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let revoked_browser_sessions = sqlx::query(
        "UPDATE browser_sessions \
         SET revoked_at = now(), revoked_reason = 'account_deleted', updated_at = now() \
         WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let disconnected_connections = sqlx::query(
        "UPDATE space_agent_connections \
         SET disconnected_at = now(), disconnected_by_user_id = $2 \
         WHERE disconnected_at IS NULL \
           AND agent_id = ANY($1)",
    )
    .bind(&owned_agents)
    .bind(deleted_by)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    // Soft-delete only: keep display_name and the users-row PII/`provider_sub_hash`
    // as a tombstone. The purge run anonymizes PII and frees the sub-hash later.
    sqlx::query(
        "UPDATE accounts \
         SET is_active = false, deleted_at = now(), deleted_by_account_id = $2, updated_at = now() \
         WHERE id = $1 AND kind = 'user'",
    )
    .bind(account_id)
    .bind(deleted_by)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let counts = AccountDeleteCounts {
        deactivated_agents: deactivated_agents.rows_affected(),
        revoked_api_keys: revoked_api_keys.rows_affected(),
        revoked_browser_sessions: revoked_browser_sessions.rows_affected(),
        disconnected_connections: disconnected_connections.rows_affected(),
    };

    let audit_ctx = AuditContext::rest(deleted_by);
    audit_events::account_deleted(&mut tx, audit_ctx, account_id, counts).await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok(())
}
