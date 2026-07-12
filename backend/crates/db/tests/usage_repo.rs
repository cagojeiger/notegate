//! Integration tests for user usage views and manual reconciliation requests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use chrono::{DateTime, Utc};
use common::{TestDb, insert_user_account, set_user_tier, space_with_root};
use notegate_core::Error;
use notegate_core::tier::UserTier;
use notegate_db::UsageRepo;
use uuid::Uuid;

#[tokio::test]
async fn current_user_usage_reads_counters_and_live_related_resources()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (user_id, space_id, _) = space_with_root(&db.pool, "usage-view").await?;
    set_user_tier(&db.pool, user_id, "tier0").await?;
    let reconciled_at: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE space_usage SET live_node_count = 7, live_content_bytes = 123 \
         WHERE space_id = $1 RETURNING reconciled_at",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;

    let live_agent_id: Uuid =
        sqlx::query_scalar("INSERT INTO accounts (kind) VALUES ('agent') RETURNING id")
            .fetch_one(&db.pool)
            .await?;
    sqlx::query("INSERT INTO agents (id, owner_user_id, name) VALUES ($1, $2, 'live-agent')")
        .bind(live_agent_id)
        .bind(user_id)
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "INSERT INTO space_agent_connections \
         (space_id, agent_id, permission, connected_by_user_id) \
         VALUES ($1, $2, 'write', $3)",
    )
    .bind(space_id)
    .bind(live_agent_id)
    .bind(user_id)
    .execute(&db.pool)
    .await?;

    let inactive_agent_id: Uuid = sqlx::query_scalar(
        "INSERT INTO accounts (kind, is_active) VALUES ('agent', false) RETURNING id",
    )
    .fetch_one(&db.pool)
    .await?;
    sqlx::query("INSERT INTO agents (id, owner_user_id, name) VALUES ($1, $2, 'inactive-agent')")
        .bind(inactive_agent_id)
        .bind(user_id)
        .execute(&db.pool)
        .await?;

    sqlx::query(
        "INSERT INTO api_keys \
         (account_id, created_by_user_id, name, token_prefix, token_hash, hash_key_id, expires_at) \
         VALUES \
         ($1, $1, 'live', 'ngk_live', $2, 'test-lookup', now() + interval '1 day'), \
         ($1, $1, 'expired', 'ngk_expired', $3, 'test-lookup', now() - interval '1 day'), \
         ($1, $1, 'revoked', 'ngk_revoked', $4, 'test-lookup', now() + interval '1 day')",
    )
    .bind(user_id)
    .bind(format!("live-{space_id}"))
    .bind(format!("expired-{space_id}"))
    .bind(format!("revoked-{space_id}"))
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "UPDATE api_keys SET revoked_at = now(), revoked_reason = 'test' \
         WHERE account_id = $1 AND name = 'revoked'",
    )
    .bind(user_id)
    .execute(&db.pool)
    .await?;

    let snapshot = UsageRepo::new(db.pool.clone())
        .current_user_usage(user_id)
        .await?
        .expect("live user usage");
    assert_eq!(snapshot.tier, UserTier::Tier0);
    assert_eq!(snapshot.live_agents, 1);
    assert_eq!(snapshot.live_api_keys, 1);
    assert_eq!(snapshot.spaces.len(), 1);
    let space = &snapshot.spaces[0];
    assert_eq!(space.id, space_id);
    assert_eq!(space.live_nodes, 7);
    assert_eq!(space.live_content_bytes, 123);
    assert_eq!(space.live_agent_connections, 1);
    assert_eq!(space.reconciled_at, reconciled_at);
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
    assert_eq!(
        usize::from(first_result.is_ok()) + usize::from(second_result.is_ok()),
        1
    );
    for error in [first_result, second_result]
        .into_iter()
        .filter_map(Result::err)
    {
        assert!(matches!(error, Error::Conflict(_)));
    }

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
    let error = repo
        .request_space_reconciliation(owner_user_id, space_id)
        .await
        .expect_err("recent reconciliation must enforce cooldown");
    assert!(matches!(error, Error::Conflict(_)));

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
