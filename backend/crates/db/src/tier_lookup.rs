//! User tier lookup helpers shared by repositories.

use notegate_core::tier::UserTier;
use notegate_core::{Error, Result};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::map_sqlx_error;

/// Lock and load the tier of a live user account inside a mutation transaction.
pub(crate) async fn lock_active_user_tier(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    not_found_message: &'static str,
) -> Result<UserTier> {
    let tier: Option<String> = sqlx::query_scalar(
        "SELECT u.tier FROM users u \
         JOIN accounts acc ON acc.id = u.id \
         WHERE u.id = $1 AND acc.kind = 'user' AND acc.is_active = true AND acc.deleted_at IS NULL \
         FOR UPDATE OF acc",
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;

    tier.as_deref()
        .map(UserTier::parse_db)
        .transpose()?
        .ok_or_else(|| Error::not_found(not_found_message))
}

/// Lock and load the tier of a live space owner inside a mutation transaction.
pub(crate) async fn lock_active_space_owner_tier(
    tx: &mut PgConnection,
    space_id: Uuid,
    not_found_message: &'static str,
) -> Result<UserTier> {
    let tier: Option<String> = sqlx::query_scalar(
        "SELECT u.tier FROM spaces s \
         JOIN users u ON u.id = s.owner_user_id \
         JOIN accounts acc ON acc.id = u.id \
         WHERE s.id = $1 AND s.deleted_at IS NULL \
           AND acc.kind = 'user' AND acc.is_active = true AND acc.deleted_at IS NULL \
         FOR UPDATE OF acc",
    )
    .bind(space_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    tier.as_deref()
        .map(UserTier::parse_db)
        .transpose()?
        .ok_or_else(|| Error::not_found(not_found_message))
}

/// Load the tier of a live space owner for read/query paths.
pub(crate) async fn active_space_owner_tier(pool: &PgPool, space_id: Uuid) -> Result<UserTier> {
    let tier: String = sqlx::query_scalar(
        "SELECT u.tier FROM spaces s \
         JOIN users u ON u.id = s.owner_user_id \
         JOIN accounts acc ON acc.id = u.id \
         WHERE s.id = $1 AND s.deleted_at IS NULL \
           AND acc.kind = 'user' AND acc.is_active = true AND acc.deleted_at IS NULL",
    )
    .bind(space_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;

    UserTier::parse_db(&tier)
}
