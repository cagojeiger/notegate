//! Shared workspace role resolution for read authorization.

use crate::map_sqlx_error;
use notegate_core::{Error, Result};
use notegate_model::Role;
use sqlx::PgPool;
use uuid::Uuid;

/// The caller's effective role in a live workspace, or `None` if the workspace
/// is hidden/deleted or the caller has no live grant from an active account.
///
/// Agent accounts may receive editor/viewer grants, but an `owner` grant is only
/// effective for user accounts.
pub(crate) async fn live_role(
    pool: &PgPool,
    workspace_id: Uuid,
    account_id: Uuid,
) -> Result<Option<Role>> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT wa.role \
         FROM workspaces w \
         JOIN workspace_access wa ON wa.workspace_id = w.id \
                                 AND wa.account_id = $2 \
                                 AND wa.revoked_at IS NULL \
         JOIN accounts caller ON caller.id = wa.account_id \
                              AND caller.is_active = true \
                              AND caller.deleted_at IS NULL \
         WHERE w.id = $1 AND w.deleted_at IS NULL \
           AND (wa.role <> 'owner' OR caller.kind = 'user')",
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
