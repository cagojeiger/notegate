//! Integration tests for queued Space usage reconciliation and mutation gates.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use chrono::Utc;
use common::{TestDb, attach_file, space_with_root};
use notegate_core::Error;
use notegate_db::{FilesRepo, SpaceRepo, SpaceUsageRepo, UsageCounts, UsageReconcileExecution};
use notegate_model::files::{CreateFolder, StoredContent, WriteTextBody};
use serde_json::{Value, json};
use uuid::Uuid;

const RECONCILE_ADVISORY_LOCK_SEED: i64 = 0x4e47_5553_4147_4501;
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

fn space_gate_seed(space_id: Uuid) -> i64 {
    let value = space_id.as_u128();
    let folded = (value as u64) ^ ((value >> 64) as u64) ^ SPACE_GATE_NAMESPACE;
    i64::from_ne_bytes(folded.to_ne_bytes())
}

async fn acquire_gate(
    tx: &mut sqlx::PgConnection,
    seed: i64,
    shared: bool,
) -> Result<(), sqlx::Error> {
    let query = if shared {
        "SELECT pg_advisory_xact_lock_shared(hashtextextended(current_schema(), $1))"
    } else {
        "SELECT pg_advisory_xact_lock(hashtextextended(current_schema(), $1))"
    };
    sqlx::query(query).bind(seed).execute(tx).await?;
    Ok(())
}

async fn queue_job(pool: &sqlx::PgPool, space_id: Uuid) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO space_usage_reconcile_jobs (space_id) VALUES ($1) RETURNING job_id",
    )
    .bind(space_id)
    .fetch_one(pool)
    .await
}

#[tokio::test]
async fn reconciliation_repairs_drift_and_records_execution()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "reconcile-drift").await?;
    let files = FilesRepo::new(db.pool.clone());
    files
        .insert_text(space_id, root_id, "note.md", &text("hello"), account)
        .await?;
    attach_file(&files, space_id, root_id, "asset.bin", 3, account).await?;
    sqlx::query(
        "UPDATE space_usage \
         SET live_node_count = 99, live_text_bytes = 999, live_file_bytes = 888, \
             reconciled_at = now() - interval '30 days' \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;
    let job_id = queue_job(&db.pool, space_id).await?;

    let execution = SpaceUsageRepo::new(db.pool.clone())
        .execute_next_reconciliation()
        .await?;
    assert_eq!(
        execution,
        UsageReconcileExecution::Succeeded {
            job_id,
            space_id,
            previous: Some(UsageCounts {
                live_node_count: 99,
                live_text_bytes: 999,
                live_file_bytes: 888,
            }),
            actual: UsageCounts {
                live_node_count: 3,
                live_text_bytes: 5,
                live_file_bytes: 3,
            },
        }
    );

    let stored: (i64, i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_text_bytes, live_file_bytes \
         FROM space_usage WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(stored, (3, 5, 3));
    let (outcome, metadata): (String, Value) = sqlx::query_as(
        "SELECT outcome, metadata FROM space_usage_reconcile_executions WHERE job_id = $1",
    )
    .bind(job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(outcome, "succeeded");
    assert_eq!(metadata["actual_nodes"], json!(3));
    assert_eq!(metadata["actual_text_bytes"], json!(5));
    assert_eq!(metadata["actual_file_bytes"], json!(3));
    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .execute_next_reconciliation()
            .await?,
        UsageReconcileExecution::Idle
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn reconciliation_cancels_a_job_for_a_deleted_space() -> Result<(), Box<dyn std::error::Error>>
{
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, _) = space_with_root(&db.pool, "reconcile-deleted").await?;
    let job_id = queue_job(&db.pool, space_id).await?;
    SpaceRepo::new(db.pool.clone())
        .delete_space(space_id, account, account)
        .await?;

    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .execute_next_reconciliation()
            .await?,
        UsageReconcileExecution::Cancelled { job_id, space_id }
    );

    let job_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM space_usage_reconcile_jobs WHERE job_id = $1)",
    )
    .bind(job_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(!job_exists);
    let (outcome, metadata): (String, Value) = sqlx::query_as(
        "SELECT outcome, metadata FROM space_usage_reconcile_executions WHERE job_id = $1",
    )
    .bind(job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(outcome, "cancelled");
    assert_eq!(metadata["reason"], json!("space_deleted"));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn bulk_enqueue_reconciles_every_live_space() -> Result<(), Box<dyn std::error::Error>> {
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
        "UPDATE space_usage \
         SET live_node_count = 99, live_text_bytes = 999, live_file_bytes = 888, \
         reconciled_at = now() - interval '30 days'",
    )
    .execute(&db.pool)
    .await?;
    // A pre-existing deferred job must become runnable again, not duplicate.
    let queued_job_id = queue_job(&db.pool, first_space).await?;
    sqlx::query(
        "UPDATE space_usage_reconcile_jobs SET run_after = now() + interval '5 minutes' \
         WHERE job_id = $1",
    )
    .bind(queued_job_id)
    .execute(&db.pool)
    .await?;

    let repo = SpaceUsageRepo::new(db.pool.clone());
    assert_eq!(repo.enqueue_all_live_spaces().await?, 2);

    let mut reconciled = Vec::new();
    loop {
        match repo.execute_next_reconciliation().await? {
            UsageReconcileExecution::Succeeded { space_id, .. } => reconciled.push(space_id),
            UsageReconcileExecution::Idle => break,
            other => panic!("unexpected execution during drain: {other:?}"),
        }
    }
    reconciled.sort();
    let mut expected = vec![first_space, second_space];
    expected.sort();
    assert_eq!(reconciled, expected);

    let first: (i64, i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_text_bytes, live_file_bytes \
         FROM space_usage WHERE space_id = $1",
    )
    .bind(first_space)
    .fetch_one(&db.pool)
    .await?;
    let second: (i64, i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_text_bytes, live_file_bytes \
         FROM space_usage WHERE space_id = $1",
    )
    .bind(second_space)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(first, (2, 5, 0));
    assert_eq!(second, (1, 0, 0));
    let queued_jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM space_usage_reconcile_jobs")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(queued_jobs, 0);
    let (outcome, metadata): (String, Value) = sqlx::query_as(
        "SELECT outcome, metadata FROM space_usage_reconcile_executions WHERE job_id = $1",
    )
    .bind(queued_job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(outcome, "succeeded");
    assert_eq!(metadata["previous_nodes"], json!(99));
    assert_eq!(metadata["actual_nodes"], json!(2));

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
    queue_job(&db.pool, space_id).await?;

    let mut lock_tx = db.pool.begin().await?;
    acquire_gate(&mut lock_tx, RECONCILE_ADVISORY_LOCK_SEED, false).await?;

    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .execute_next_reconciliation()
            .await?,
        UsageReconcileExecution::WorkerLockHeld
    );

    lock_tx.commit().await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn busy_job_is_deferred_without_blocking_the_next_job()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, busy_space_id, _) = space_with_root(&db.pool, "reconcile-busy").await?;
    let (_, next_space_id, _) = space_with_root(&db.pool, "reconcile-next").await?;
    let busy_job_id = queue_job(&db.pool, busy_space_id).await?;
    let next_job_id = queue_job(&db.pool, next_space_id).await?;
    sqlx::query(
        "UPDATE space_usage_reconcile_jobs \
         SET run_after = CASE space_id \
             WHEN $1 THEN now() - interval '2 hours' \
             ELSE now() - interval '1 hour' END \
         WHERE space_id IN ($1, $2)",
    )
    .bind(busy_space_id)
    .bind(next_space_id)
    .execute(&db.pool)
    .await?;

    let mut mutation_tx = db.pool.begin().await?;
    acquire_gate(&mut mutation_tx, space_gate_seed(busy_space_id), true).await?;

    let execution = SpaceUsageRepo::new(db.pool.clone())
        .execute_next_reconciliation()
        .await?;
    assert!(matches!(
        execution,
        UsageReconcileExecution::Deferred { job_id, space_id, run_after }
            if job_id == busy_job_id && space_id == busy_space_id && run_after > Utc::now()
    ));
    let (outcome, retry_count): (String, i32) = sqlx::query_as(
        "SELECT e.outcome, j.retry_count \
         FROM space_usage_reconcile_executions e \
         JOIN space_usage_reconcile_jobs j ON j.job_id = e.job_id \
         WHERE e.job_id = $1",
    )
    .bind(busy_job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!((outcome.as_str(), retry_count), ("deferred", 0));

    assert!(matches!(
        SpaceUsageRepo::new(db.pool.clone())
            .execute_next_reconciliation()
            .await?,
        UsageReconcileExecution::Succeeded { job_id, space_id, .. }
            if job_id == next_job_id && space_id == next_space_id
    ));

    mutation_tx.commit().await?;
    sqlx::query("UPDATE space_usage_reconcile_jobs SET run_after = now() WHERE job_id = $1")
        .bind(busy_job_id)
        .execute(&db.pool)
        .await?;
    assert!(matches!(
        SpaceUsageRepo::new(db.pool.clone())
            .execute_next_reconciliation()
            .await?,
        UsageReconcileExecution::Succeeded { job_id, space_id, .. }
            if job_id == busy_job_id && space_id == busy_space_id
    ));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn failed_execution_is_recorded_and_retried_later() -> Result<(), Box<dyn std::error::Error>>
{
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, _) = space_with_root(&db.pool, "reconcile-failure").await?;
    let job_id = queue_job(&db.pool, space_id).await?;
    // A live Space without any live node is corrupt: the exact count of 0
    // violates the counter CHECK, so the execution must fail loudly.
    sqlx::query("DELETE FROM nodes WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    let execution = SpaceUsageRepo::new(db.pool.clone())
        .execute_next_reconciliation()
        .await?;
    assert!(matches!(
        execution,
        UsageReconcileExecution::Failed { job_id: id, space_id: failed_space, run_after, ref error }
            if id == job_id && failed_space == space_id && run_after > Utc::now()
                && error.contains("database query failed")
    ));
    let (retry_count, outcome, has_error): (i32, String, bool) = sqlx::query_as(
        "SELECT j.retry_count, e.outcome, e.error_message IS NOT NULL \
         FROM space_usage_reconcile_jobs j \
         JOIN space_usage_reconcile_executions e ON e.job_id = j.job_id \
         WHERE j.job_id = $1",
    )
    .bind(job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        (retry_count, outcome.as_str(), has_error),
        (1, "failed", true)
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn missing_counter_is_recreated_by_reconciliation() -> Result<(), Box<dyn std::error::Error>>
{
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, _) = space_with_root(&db.pool, "reconcile-missing-counter").await?;
    let job_id = queue_job(&db.pool, space_id).await?;
    sqlx::query("DELETE FROM space_usage WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    let execution = SpaceUsageRepo::new(db.pool.clone())
        .execute_next_reconciliation()
        .await?;
    assert_eq!(
        execution,
        UsageReconcileExecution::Succeeded {
            job_id,
            space_id,
            previous: None,
            actual: UsageCounts {
                live_node_count: 1,
                live_text_bytes: 0,
                live_file_bytes: 0,
            },
        }
    );

    let stored: (i64, i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_text_bytes, live_file_bytes \
         FROM space_usage WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(stored, (1, 0, 0));
    let metadata: Value = sqlx::query_scalar(
        "SELECT metadata FROM space_usage_reconcile_executions WHERE job_id = $1",
    )
    .bind(job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(metadata["previous_nodes"], Value::Null);
    assert_eq!(metadata["actual_nodes"], json!(1));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn expired_execution_history_is_removed() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = RECONCILIATION_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, _) = space_with_root(&db.pool, "reconcile-retention").await?;
    sqlx::query(
        "INSERT INTO space_usage_reconcile_executions ( \
             job_id, space_id, started_at, finished_at, outcome \
         ) VALUES (gen_random_uuid(), $1, now() - interval '4 months', \
                   now() - interval '4 months', 'succeeded')",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    let mut worker_tx = db.pool.begin().await?;
    acquire_gate(&mut worker_tx, RECONCILE_ADVISORY_LOCK_SEED, false).await?;
    assert!(
        !SpaceUsageRepo::new(db.pool.clone())
            .try_delete_expired_executions()
            .await?
    );
    let retained: i64 = sqlx::query_scalar("SELECT count(*) FROM space_usage_reconcile_executions")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(retained, 1);
    worker_tx.commit().await?;

    assert!(
        SpaceUsageRepo::new(db.pool.clone())
            .try_delete_expired_executions()
            .await?
    );
    let executions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM space_usage_reconcile_executions")
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(executions, 0);

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
    acquire_gate(&mut reconciliation_tx, space_gate_seed(space_id), false).await?;

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
