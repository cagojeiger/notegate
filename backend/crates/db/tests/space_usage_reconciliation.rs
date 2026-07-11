//! Integration tests for distributed Space usage reconciliation and mutation gates.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use chrono::{Duration, Utc};
use common::{TestDb, space_with_root};
use notegate_core::Error;
use notegate_db::{
    FilesRepo, FullUsageReconcileRun, SpaceUsageRepo, UsageCounts, UsageReconcileRun,
};
use notegate_model::files::{CreateFolder, StoredContent, WriteTextBody};
use uuid::Uuid;

const RECONCILE_ADVISORY_LOCK_KEY: i64 = 0x4e47_5553_4147_4501;
const FULL_RECONCILIATION_GATE_KEY: i64 = 0x4e47_5553_4147_4502;
const SPACE_GATE_NAMESPACE: u64 = 0x4e47_5350_4143_4501;
static RECONCILIATION_TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn text(content: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: "0".repeat(64),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

fn space_gate_key(space_id: Uuid) -> i64 {
    let value = space_id.as_u128();
    let folded = (value as u64) ^ ((value >> 64) as u64) ^ SPACE_GATE_NAMESPACE;
    i64::from_ne_bytes(folded.to_ne_bytes())
}

async fn mark_due(pool: &sqlx::PgPool, space_id: Uuid, age: Duration) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE space_usage SET next_reconcile_at = now() - $2 WHERE space_id = $1")
        .bind(space_id)
        .bind(age)
        .execute(pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn reconciliation_repairs_drift_and_schedules_the_next_run()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "reconcile-drift").await?;
    FilesRepo::new(db.pool.clone())
        .insert_text(space_id, root_id, "note.md", &text("hello"), account)
        .await?;
    sqlx::query(
        "UPDATE space_usage \
         SET live_node_count = 99, live_content_bytes = 999, \
             reconciled_at = now() - interval '30 days', \
             next_reconcile_at = now() - interval '1 hour' \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    let started_at = Utc::now();
    let run = SpaceUsageRepo::new(db.pool.clone())
        .run_reconciliation_once()
        .await?;
    let UsageReconcileRun::Reconciled {
        space_id: reconciled_space_id,
        previous,
        actual,
        next_reconcile_at,
        ..
    } = run
    else {
        panic!("expected reconciliation, got {run:?}");
    };
    assert_eq!(reconciled_space_id, space_id);
    assert_eq!(
        previous,
        UsageCounts {
            live_node_count: 99,
            live_content_bytes: 999,
        }
    );
    assert_eq!(
        actual,
        UsageCounts {
            live_node_count: 2,
            live_content_bytes: 5,
        }
    );
    assert!(next_reconcile_at > started_at + Duration::days(6));
    assert!(next_reconcile_at < Utc::now() + Duration::days(8));

    let stored: (i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_content_bytes FROM space_usage WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(stored, (2, 5));
    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .run_reconciliation_once()
            .await?,
        UsageReconcileRun::Idle
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn full_recalculation_repairs_every_live_space_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (first_account, first_space, first_root) =
        space_with_root(&db.pool, "full-reconcile-first").await?;
    let (_, second_space, _) = space_with_root(&db.pool, "full-reconcile-second").await?;
    FilesRepo::new(db.pool.clone())
        .insert_text(
            first_space,
            first_root,
            "note.md",
            &text("hello"),
            first_account,
        )
        .await?;
    sqlx::query(
        "UPDATE space_usage SET live_node_count = 99, live_content_bytes = 999, \
         reconciled_at = now() - interval '30 days'",
    )
    .execute(&db.pool)
    .await?;

    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .run_full_recalculation()
            .await?,
        FullUsageReconcileRun::Recalculated {
            spaces_recalculated: 2
        }
    );

    let first: (i64, i64, bool) = sqlx::query_as(
        "SELECT live_node_count, live_content_bytes, next_reconcile_at > now() \
         FROM space_usage WHERE space_id = $1",
    )
    .bind(first_space)
    .fetch_one(&db.pool)
    .await?;
    let second: (i64, i64, bool) = sqlx::query_as(
        "SELECT live_node_count, live_content_bytes, next_reconcile_at > now() \
         FROM space_usage WHERE space_id = $1",
    )
    .bind(second_space)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(first, (2, 5, true));
    assert_eq!(second, (1, 0, true));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn full_recalculation_skips_while_mutations_are_active()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    space_with_root(&db.pool, "full-reconcile-busy").await?;
    let mut mutation_tx = db.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock_shared($1)")
        .bind(FULL_RECONCILIATION_GATE_KEY)
        .execute(&mut *mutation_tx)
        .await?;

    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .run_full_recalculation()
            .await?,
        FullUsageReconcileRun::MutationsActive
    );

    mutation_tx.commit().await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn reconciliation_skips_when_another_worker_holds_the_database_lock()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, _) = space_with_root(&db.pool, "reconcile-worker-lock").await?;
    mark_due(&db.pool, space_id, Duration::hours(1)).await?;

    let mut lock_tx = db.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(RECONCILE_ADVISORY_LOCK_KEY)
        .execute(&mut *lock_tx)
        .await?;

    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .run_reconciliation_once()
            .await?,
        UsageReconcileRun::LockHeld
    );

    lock_tx.commit().await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn reconciliation_skips_busy_spaces_without_starving_the_next_candidate()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, busy_space_id, _) = space_with_root(&db.pool, "reconcile-busy").await?;
    let (_, next_space_id, _) = space_with_root(&db.pool, "reconcile-next").await?;
    mark_due(&db.pool, busy_space_id, Duration::hours(2)).await?;

    let mut mutation_tx = db.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock_shared($1)")
        .bind(space_gate_key(busy_space_id))
        .execute(&mut *mutation_tx)
        .await?;

    let run = SpaceUsageRepo::new(db.pool.clone())
        .run_reconciliation_once()
        .await?;
    assert!(matches!(
        run,
        UsageReconcileRun::SpacesBusy {
            oldest_space_id,
            candidates_checked: 1,
            ..
        } if oldest_space_id == busy_space_id
    ));

    mark_due(&db.pool, next_space_id, Duration::hours(1)).await?;
    let run = SpaceUsageRepo::new(db.pool.clone())
        .run_reconciliation_once()
        .await?;
    assert!(matches!(
        run,
        UsageReconcileRun::Reconciled { space_id, .. } if space_id == next_space_id
    ));

    mutation_tx.commit().await?;
    let run = SpaceUsageRepo::new(db.pool.clone())
        .run_reconciliation_once()
        .await?;
    assert!(matches!(
        run,
        UsageReconcileRun::Reconciled { space_id, .. } if space_id == busy_space_id
    ));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn exclusive_reconciliation_gate_rejects_then_releases_mutations()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "reconcile-mutation").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let command = CreateFolder {
        parent_node_id: root_id,
        name: "blocked".to_owned(),
    };

    let mut reconciliation_tx = db.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(space_gate_key(space_id))
        .execute(&mut *reconciliation_tx)
        .await?;

    let error = repo
        .insert_folder(space_id, &command, account)
        .await
        .expect_err("mutation must fail while reconciliation holds the gate");
    assert!(matches!(
        error,
        Error::UsageRecalculationInProgress {
            retry_after_seconds: 5
        }
    ));
    let usage_repo = SpaceUsageRepo::new(db.pool.clone());
    assert_eq!(
        usage_repo
            .calculate_exact_usage(space_id)
            .await?
            .live_node_count,
        1
    );

    reconciliation_tx.commit().await?;
    repo.insert_folder(space_id, &command, account).await?;
    assert_eq!(
        usage_repo
            .calculate_exact_usage(space_id)
            .await?
            .live_node_count,
        2
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn full_recalculation_gate_rejects_then_releases_mutations()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "full-reconcile-mutation").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let command = CreateFolder {
        parent_node_id: root_id,
        name: "blocked".to_owned(),
    };

    let mut reconciliation_tx = db.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(FULL_RECONCILIATION_GATE_KEY)
        .execute(&mut *reconciliation_tx)
        .await?;

    let error = repo
        .insert_folder(space_id, &command, account)
        .await
        .expect_err("mutation must fail while full recalculation holds the gate");
    assert!(matches!(
        error,
        Error::UsageRecalculationInProgress {
            retry_after_seconds: 5
        }
    ));

    reconciliation_tx.commit().await?;
    repo.insert_folder(space_id, &command, account).await?;

    db.cleanup().await;
    Ok(())
}
