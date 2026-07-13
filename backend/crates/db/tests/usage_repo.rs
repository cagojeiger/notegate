//! Integration tests for user usage views and manual reconciliation requests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account, set_user_tier, space_with_root};
use notegate_core::Error;
use notegate_core::tier::UserTier;
use notegate_db::{UsageReconciliationOutcome, UsageRepo};
use uuid::Uuid;

#[tokio::test]
async fn current_user_usage_reads_space_counters() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (user_id, space_id, _) = space_with_root(&db.pool, "usage-view").await?;
    set_user_tier(&db.pool, user_id, "tier0").await?;
    sqlx::query(
        "UPDATE space_usage \
         SET live_node_count = 7, live_text_bytes = 123, live_file_bytes = 45 \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    let snapshot = UsageRepo::new(db.pool.clone())
        .current_user_usage(user_id)
        .await?
        .expect("live user usage");
    assert_eq!(snapshot.tier, UserTier::Tier0);
    assert_eq!(snapshot.spaces.len(), 1);
    let space = &snapshot.spaces[0];
    assert_eq!(space.id, space_id);
    assert_eq!(space.live_nodes, 7);
    assert_eq!(space.live_text_bytes, 123);
    assert_eq!(space.live_file_bytes, 45);
    assert!(!space.reconciliation_pending);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn reconciliation_request_allows_only_one_concurrent_queue()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner_user_id, space_id, _) = space_with_root(&db.pool, "usage-request-race").await?;
    make_reconciliation_requestable(&db.pool, space_id).await?;

    let first = UsageRepo::new(db.pool.clone());
    let second = first.clone();
    let (first_result, second_result) = tokio::join!(
        first.request_space_reconciliation(owner_user_id, space_id),
        second.request_space_reconciliation(owner_user_id, space_id),
    );
    let outcomes = [first_result?, second_result?];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == UsageReconciliationOutcome::Queued)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == UsageReconciliationOutcome::AlreadyQueued)
            .count(),
        1
    );

    let queued: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM space_usage_reconcile_jobs WHERE space_id = $1 \
         )",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(queued);

    let snapshot = UsageRepo::new(db.pool.clone())
        .current_user_usage(owner_user_id)
        .await?
        .expect("live user usage");
    assert!(snapshot.spaces[0].reconciliation_pending);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn current_user_usage_rejects_a_missing_space_counter()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (user_id, space_id, _) = space_with_root(&db.pool, "usage-missing-counter").await?;
    sqlx::query("DELETE FROM space_usage WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    let error = UsageRepo::new(db.pool.clone())
        .current_user_usage(user_id)
        .await
        .expect_err("missing usage counter must not silently hide a live space");
    assert!(matches!(error, Error::Internal(_)));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn reconciliation_request_enforces_owner_and_cooldown()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner_user_id, space_id, _) = space_with_root(&db.pool, "usage-request-guard").await?;
    let other_user_id =
        insert_user_account(&db.pool, "usage-other", "usage-other@example.com").await?;
    let repo = UsageRepo::new(db.pool.clone());

    let error = repo
        .request_space_reconciliation(other_user_id, space_id)
        .await
        .expect_err("non-owner request must fail");
    assert!(matches!(error, Error::NotFound(_)));

    sqlx::query(
        "UPDATE space_usage \
         SET reconciled_at = now() \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;
    let outcome = repo
        .request_space_reconciliation(owner_user_id, space_id)
        .await?;
    assert_eq!(outcome, UsageReconciliationOutcome::Cooldown);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn reconciliation_request_bounds_space_lock_waits() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner_user_id, space_id, _) = space_with_root(&db.pool, "usage-request-lock").await?;
    let mut lock_tx = db.pool.begin().await?;
    sqlx::query("SELECT id FROM spaces WHERE id = $1 FOR UPDATE")
        .bind(space_id)
        .execute(&mut *lock_tx)
        .await?;

    let error = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        UsageRepo::new(db.pool.clone()).request_space_reconciliation(owner_user_id, space_id),
    )
    .await
    .expect("request must return before the HTTP timeout")
    .expect_err("locked Space must return a retryable error");
    assert!(matches!(
        error,
        Error::UsageRecalculationInProgress {
            retry_after_seconds: 2
        }
    ));

    lock_tx.rollback().await?;
    db.cleanup().await;
    Ok(())
}

async fn make_reconciliation_requestable(
    pool: &sqlx::PgPool,
    space_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE space_usage \
         SET reconciled_at = now() - interval '2 hours' \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .execute(pool)
    .await?;
    Ok(())
}
