//! Shared space permission resolution.

use crate::map_sqlx_error;
use notegate_core::{Error, Result};
use notegate_model::Permission;
use sqlx::PgPool;
use uuid::Uuid;

/// Effective permission in a live space for a live account.
pub(crate) async fn permission_for(
    pool: &PgPool,
    space_id: Uuid,
    account_id: Uuid,
) -> Result<Option<Permission>> {
    let permission: Option<String> = sqlx::query_scalar(
        "SELECT CASE \
             WHEN acc.kind = 'user' AND s.owner_user_id = acc.id THEN 'write' \
             WHEN acc.kind = 'agent' THEN c.permission \
             ELSE NULL \
         END AS permission \
         FROM accounts acc \
         JOIN spaces s ON s.id = $1 AND s.deleted_at IS NULL \
         LEFT JOIN space_agent_connections c \
           ON c.space_id = s.id AND c.agent_id = acc.id AND c.disconnected_at IS NULL \
         WHERE acc.id = $2 AND acc.is_active = true AND acc.deleted_at IS NULL \
           AND ( \
             (acc.kind = 'user' AND s.owner_user_id = acc.id) \
             OR (acc.kind = 'agent' AND c.agent_id IS NOT NULL) \
           )",
    )
    .bind(space_id)
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    permission
        .map(|value| {
            Permission::parse(&value)
                .ok_or_else(|| Error::internal(format!("unknown space permission: {value}")))
        })
        .transpose()
}
