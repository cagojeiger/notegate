//! Shared helpers for DB integration tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
use notegate_db::AccountRepo;
pub use notegate_db::test_support::TestDb;
use notegate_model::ResolveAttrs;
use sqlx::PgPool;
use uuid::Uuid;

/// Insert a `kind='user'` account + matching `users` row, returning the id.
///
/// Shared by the agent tests; `account_repo` builds its users through the repo,
/// so the lint fires only for that binary.
#[allow(dead_code)]
pub async fn insert_user_account(
    pool: &PgPool,
    sub: &str,
    email: &str,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let (account, _) = AccountRepo::new(pool.clone())
        .upsert_user_by_sub(&ResolveAttrs {
            sub: sub.to_owned(),
            email: email.to_owned(),
            name: format!("user-{sub}"),
        })
        .await?;
    Ok(account.id)
}

/// Assign a test user to a product tier.
#[allow(dead_code)]
pub async fn set_user_tier(pool: &PgPool, user_id: Uuid, tier: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET tier = $2 WHERE id = $1")
        .bind(user_id)
        .bind(tier)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deactivate an account as a soft-delete, matching the production account lifecycle.
#[allow(dead_code)]
pub async fn deactivate_account(
    pool: &PgPool,
    account_id: Uuid,
    deleted_by: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE accounts \
         SET is_active = false, deleted_at = now(), deleted_by_account_id = $2, updated_at = now() \
         WHERE id = $1",
    )
    .bind(account_id)
    .bind(deleted_by)
    .execute(pool)
    .await?;
    Ok(())
}
