//! Integration tests for the shared desired-state reconciliation queue.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use std::time::Duration;

use common::{TestDb, space_with_root};
use notegate_db::ReconciliationRepo;

#[tokio::test]
async fn enqueue_coalesces_generations_and_backlog_counts_only_actionable_work()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, root_id) = space_with_root(&db.pool, "reconciliation-coalesce").await?;
    sqlx::query("DELETE FROM reconciliation_work_items WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    for _ in 0..2 {
        sqlx::query_scalar::<_, bool>(
            "SELECT enqueue_reconciliation_work('test', 'document', $1, $2)",
        )
        .bind(space_id)
        .bind(root_id)
        .fetch_one(&db.pool)
        .await?;
    }
    sqlx::query_scalar::<_, bool>(
        "SELECT enqueue_reconciliation_work('other', 'document', $1, $2)",
    )
    .bind(space_id)
    .bind(root_id)
    .fetch_one(&db.pool)
    .await?;

    let state: (i64, i64) = sqlx::query_as(
        "SELECT requested_generation, applied_generation \
         FROM reconciliation_work_items \
         WHERE queue_name = 'test' AND work_kind = 'document' AND target_id = $1",
    )
    .bind(root_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(state, (2, 0));

    let work = ReconciliationRepo::new(db.pool.clone());
    assert_eq!(work.backlog("test").await?, 1);
    assert_eq!(work.backlog("other").await?, 1);
    let claim = work
        .claim_one("test", Duration::from_secs(30))
        .await?
        .expect("work should be claimable");
    assert!(work.fail(&claim, Duration::from_secs(300), "retry").await?);
    assert_eq!(work.backlog("test").await?, 0);
    sqlx::query(
        "UPDATE reconciliation_work_items SET run_after = now() - INTERVAL '1 second' \
         WHERE queue_name = 'test' AND work_kind = 'document' AND target_id = $1",
    )
    .bind(root_id)
    .execute(&db.pool)
    .await?;
    assert_eq!(work.backlog("test").await?, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_workers_claim_distinct_items() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, root_id) = space_with_root(&db.pool, "reconciliation-claims").await?;
    sqlx::query("DELETE FROM reconciliation_work_items WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    for kind in ["first", "second"] {
        sqlx::query_scalar::<_, bool>("SELECT enqueue_reconciliation_work('test', $1, $2, $3)")
            .bind(kind)
            .bind(space_id)
            .bind(root_id)
            .fetch_one(&db.pool)
            .await?;
    }

    let work = ReconciliationRepo::new(db.pool.clone());
    let left = work.clone();
    let right = work.clone();
    let (left, right) = tokio::join!(
        left.claim_one("test", Duration::from_secs(30)),
        right.claim_one("test", Duration::from_secs(30)),
    );
    let left = left?.expect("first claim");
    let right = right?.expect("second claim");
    assert_ne!(left.work_kind, right.work_kind);
    assert_ne!(left.claim_token, right.claim_token);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn completion_preserves_a_newer_generation() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, root_id) = space_with_root(&db.pool, "reconciliation-generation").await?;
    sqlx::query("DELETE FROM reconciliation_work_items WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    sqlx::query_scalar::<_, bool>("SELECT enqueue_reconciliation_work('test', 'document', $1, $2)")
        .bind(space_id)
        .bind(root_id)
        .fetch_one(&db.pool)
        .await?;

    let work = ReconciliationRepo::new(db.pool.clone());
    let claim = work
        .claim_one("test", Duration::from_secs(30))
        .await?
        .expect("claim");
    sqlx::query_scalar::<_, bool>("SELECT enqueue_reconciliation_work('test', 'document', $1, $2)")
        .bind(space_id)
        .bind(root_id)
        .fetch_one(&db.pool)
        .await?;

    let mut tx = db.pool.begin().await?;
    assert!(ReconciliationRepo::complete_in(&mut tx, &claim).await?);
    tx.commit().await?;
    let state: (i64, i64) = sqlx::query_as(
        "SELECT requested_generation, applied_generation \
         FROM reconciliation_work_items \
         WHERE queue_name = 'test' AND work_kind = 'document' AND target_id = $1",
    )
    .bind(root_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(state, (2, 1));
    assert_eq!(work.backlog("test").await?, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn failure_does_not_delay_a_newer_generation() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, root_id) = space_with_root(&db.pool, "reconciliation-newer").await?;
    sqlx::query("DELETE FROM reconciliation_work_items WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    sqlx::query_scalar::<_, bool>("SELECT enqueue_reconciliation_work('test', 'document', $1, $2)")
        .bind(space_id)
        .bind(root_id)
        .fetch_one(&db.pool)
        .await?;

    let work = ReconciliationRepo::new(db.pool.clone());
    let claim = work
        .claim_one("test", Duration::from_secs(30))
        .await?
        .expect("claim");
    sqlx::query_scalar::<_, bool>("SELECT enqueue_reconciliation_work('test', 'document', $1, $2)")
        .bind(space_id)
        .bind(root_id)
        .fetch_one(&db.pool)
        .await?;
    assert!(
        work.fail(&claim, Duration::from_secs(300), "stale generation")
            .await?
    );

    assert!(
        work.claim_one("test", Duration::from_secs(30))
            .await?
            .is_some()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn expired_claim_cannot_complete_after_reclaim() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, root_id) = space_with_root(&db.pool, "reconciliation-fencing").await?;
    sqlx::query("DELETE FROM reconciliation_work_items WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    sqlx::query_scalar::<_, bool>("SELECT enqueue_reconciliation_work('test', 'document', $1, $2)")
        .bind(space_id)
        .bind(root_id)
        .fetch_one(&db.pool)
        .await?;

    let work = ReconciliationRepo::new(db.pool.clone());
    let stale = work
        .claim_one("test", Duration::from_secs(30))
        .await?
        .expect("stale claim");
    sqlx::query(
        "UPDATE reconciliation_work_items SET lease_until = now() - INTERVAL '1 second' \
         WHERE queue_name = 'test' AND work_kind = 'document' AND target_id = $1",
    )
    .bind(root_id)
    .execute(&db.pool)
    .await?;
    let current = work
        .claim_one("test", Duration::from_secs(30))
        .await?
        .expect("replacement claim");

    let mut stale_tx = db.pool.begin().await?;
    assert!(!ReconciliationRepo::complete_in(&mut stale_tx, &stale).await?);
    stale_tx.rollback().await?;
    let mut current_tx = db.pool.begin().await?;
    assert!(ReconciliationRepo::complete_in(&mut current_tx, &current).await?);
    current_tx.commit().await?;
    assert_eq!(work.backlog("test").await?, 0);

    db.cleanup().await;
    Ok(())
}
