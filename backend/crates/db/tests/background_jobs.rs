//! Integration tests for the generic PostgreSQL background job state machine.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{TestDb, space_with_root};
use notegate_jobs::{
    AttemptOutcome, ClaimedJob, DeferTransition, FailureTransition, JobDisposition, JobFailure,
    JobHandler, JobQueue, NewJob, QueueReconciler, QueueReconcilerConfig, Worker, WorkerConfig,
};
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

struct BlockingHandler {
    started: Arc<Notify>,
    release: Arc<Semaphore>,
    runs: Arc<AtomicUsize>,
}

impl JobHandler for BlockingHandler {
    fn kind(&self) -> &'static str {
        "worker-runtime"
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(30)
    }

    fn handle<'a>(
        &'a self,
        _job: &'a ClaimedJob,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
        Box::pin(async move {
            self.runs.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            self.release
                .acquire()
                .await
                .expect("release semaphore")
                .forget();
            Ok(JobDisposition::Complete)
        })
    }
}

fn worker_config() -> WorkerConfig {
    WorkerConfig {
        concurrency: 1,
        lease: Duration::from_secs(3),
        safety_poll: Duration::from_secs(60),
        retry_base: Duration::from_millis(10),
        retry_max: Duration::from_millis(100),
    }
}

async fn wait_for_status(
    pool: &PgPool,
    job_id: uuid::Uuid,
    expected: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status: String =
                sqlx::query_scalar("SELECT status FROM background_jobs WHERE job_id = $1")
                    .bind(job_id)
                    .fetch_one(pool)
                    .await?;
            if status == expected {
                return Ok::<(), sqlx::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other(format!("job did not become {expected}")))??;
    Ok(())
}

fn job(kind: &str) -> NewJob {
    NewJob::new(kind, json!({ "subject_id": uuid::Uuid::new_v4() }))
}

#[tokio::test]
async fn worker_runtime_heartbeats_and_completes_a_job() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    let started = Arc::new(Notify::new());
    let release = Arc::new(Semaphore::new(0));
    let runs = Arc::new(AtomicUsize::new(0));
    let handler = Arc::new(BlockingHandler {
        started: started.clone(),
        release: release.clone(),
        runs: runs.clone(),
    });
    let handlers: Vec<Arc<dyn JobHandler>> = vec![handler];
    let worker = Worker::new(queue.clone(), handlers, worker_config(), "runtime-test")?;
    let enqueued = queue.enqueue(&job("worker-runtime")).await?;
    let shutdown = CancellationToken::new();
    let run_shutdown = shutdown.clone();
    let worker_task = tokio::spawn(async move { worker.run(run_shutdown).await });

    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .map_err(|_| std::io::Error::other("worker did not start the job"))?;
    tokio::time::sleep(Duration::from_millis(3_500)).await;

    let row: (String, i32, bool) = sqlx::query_as(
        "SELECT status, attempt_count, lease_until > now() \
         FROM background_jobs WHERE job_id = $1",
    )
    .bind(enqueued.job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(row, ("running".to_owned(), 1, true));
    assert_eq!(queue.recover_expired(10).await?.retried, 0);
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    release.add_permits(1);
    wait_for_status(&db.pool, enqueued.job_id, "succeeded").await?;
    shutdown.cancel();
    let joined = tokio::time::timeout(Duration::from_secs(5), worker_task)
        .await
        .map_err(|_| std::io::Error::other("worker did not stop"))?;
    let worker_result = joined?;
    worker_result?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn worker_fills_free_capacity_while_another_job_is_running()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    let started = Arc::new(Notify::new());
    let release = Arc::new(Semaphore::new(0));
    let runs = Arc::new(AtomicUsize::new(0));
    let handlers: Vec<Arc<dyn JobHandler>> = vec![Arc::new(BlockingHandler {
        started: started.clone(),
        release: release.clone(),
        runs: runs.clone(),
    })];
    let mut config = worker_config();
    config.concurrency = 2;
    let worker = Worker::new(queue.clone(), handlers, config, "capacity-test")?;
    let first = queue.enqueue(&job("worker-runtime")).await?;
    let shutdown = CancellationToken::new();
    let run_shutdown = shutdown.clone();
    let worker_task = tokio::spawn(async move { worker.run(run_shutdown).await });

    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .map_err(|_| std::io::Error::other("worker did not start the first job"))?;
    let second = queue.enqueue(&job("worker-runtime")).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while runs.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("worker left an execution slot idle"))?;

    release.add_permits(2);
    wait_for_status(&db.pool, first.job_id, "succeeded").await?;
    wait_for_status(&db.pool, second.job_id, "succeeded").await?;
    shutdown.cancel();
    let joined = tokio::time::timeout(Duration::from_secs(5), worker_task)
        .await
        .map_err(|_| std::io::Error::other("worker did not stop"))?;
    joined??;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn enqueue_participates_in_the_callers_transaction() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };

    let mut rollback = db.pool.begin().await?;
    JobQueue::enqueue_in(&mut rollback, &job("transactional")).await?;
    rollback.rollback().await?;
    let after_rollback: i64 =
        sqlx::query_scalar("SELECT count(*) FROM background_jobs WHERE job_kind = 'transactional'")
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(after_rollback, 0);

    let mut commit = db.pool.begin().await?;
    JobQueue::enqueue_in(&mut commit, &job("transactional")).await?;
    commit.commit().await?;
    let after_commit: i64 =
        sqlx::query_scalar("SELECT count(*) FROM background_jobs WHERE job_kind = 'transactional'")
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(after_commit, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn background_jobs_migration_backfills_and_mirrors_legacy_usage_jobs()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup_before(32).await? else {
        return Ok(());
    };
    let (_, backfilled_space_id, _) = space_with_root(&db.pool, "jobs-backfill").await?;
    sqlx::query("INSERT INTO space_usage_reconcile_jobs (space_id, retry_count) VALUES ($1, 3)")
        .bind(backfilled_space_id)
        .execute(&db.pool)
        .await?;

    db.apply_migration(32).await?;

    let backfilled: (String, String, i32, i32, i32) = sqlx::query_as(
        "SELECT job_kind, payload ->> 'space_id', attempt_count, failure_count, max_attempts \
         FROM background_jobs WHERE payload ->> 'space_id' = $1",
    )
    .bind(backfilled_space_id.to_string())
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        backfilled,
        (
            "space_usage_reconcile".to_owned(),
            backfilled_space_id.to_string(),
            3,
            3,
            8,
        )
    );

    let (_, mirrored_space_id, _) = space_with_root(&db.pool, "jobs-mirror").await?;
    sqlx::query("INSERT INTO space_usage_reconcile_jobs (space_id) VALUES ($1)")
        .bind(mirrored_space_id)
        .execute(&db.pool)
        .await?;
    let mirrored: (String, i32, i32) = sqlx::query_as(
        "SELECT status, attempt_count, failure_count \
         FROM background_jobs WHERE payload ->> 'space_id' = $1",
    )
    .bind(mirrored_space_id.to_string())
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(mirrored, ("queued".to_owned(), 0, 0));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_workers_claim_distinct_supported_jobs() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    queue.enqueue(&job("supported")).await?;
    queue.enqueue(&job("supported")).await?;
    queue.enqueue(&job("other")).await?;
    let kinds = vec!["supported".to_owned()];

    let left = queue.clone();
    let right = queue.clone();
    let (left, right) = tokio::join!(
        left.claim_many("left", &kinds, Duration::from_secs(30), 1),
        right.claim_many("right", &kinds, Duration::from_secs(30), 1),
    );
    let left = left?.into_iter().next().expect("left claim");
    let right = right?.into_iter().next().expect("right claim");
    assert_ne!(left.job_id, right.job_id);
    assert_eq!(left.kind, "supported");
    assert_eq!(right.kind, "supported");

    let other_status: String =
        sqlx::query_scalar("SELECT status FROM background_jobs WHERE job_kind = 'other'")
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(other_status, "queued");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn success_closes_the_claim_and_attempt() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    let enqueued = queue.enqueue(&job("success")).await?;
    let kinds = vec!["success".to_owned()];
    let claim = queue
        .claim_many("worker", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("claim");
    assert!(queue.succeed(&claim).await?);

    let row: (String, i32, Option<String>) = sqlx::query_as(
        "SELECT job.status, job.attempt_count, attempt.outcome \
         FROM background_jobs job \
         JOIN background_job_attempts attempt ON attempt.job_id = job.job_id \
         WHERE job.job_id = $1",
    )
    .bind(enqueued.job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        row,
        ("succeeded".to_owned(), 1, Some("succeeded".to_owned()))
    );
    assert!(
        queue
            .claim_many("worker", &kinds, Duration::from_secs(30), 1)
            .await?
            .is_empty()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn deferral_stops_at_the_shared_attempt_limit() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    let enqueued = queue.enqueue(&job("defer").max_attempts(2)).await?;
    let kinds = vec!["defer".to_owned()];

    let first = queue
        .claim_many("worker", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("first claim");
    assert_eq!(
        queue.defer(&first, "resource_busy", Duration::ZERO).await?,
        DeferTransition::Deferred
    );

    let queued: (String, i32, i32, i64) = sqlx::query_as(
        "SELECT status, attempt_count, failure_count, \
                (SELECT count(*) FROM background_job_attempts \
                 WHERE job_id = $1 AND outcome = 'deferred') \
         FROM background_jobs WHERE job_id = $1",
    )
    .bind(enqueued.job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(queued, ("queued".to_owned(), 1, 0, 1));

    let final_claim = queue
        .claim_many("worker", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("final claim");
    assert_eq!(
        queue
            .defer(&final_claim, "resource_busy", Duration::ZERO)
            .await?,
        DeferTransition::Dead
    );

    let completed: (String, i32, i32, i64, Option<String>) = sqlx::query_as(
        "SELECT status, attempt_count, failure_count, \
                (SELECT count(*) FROM background_job_attempts WHERE job_id = $1), \
                last_error_code \
         FROM background_jobs WHERE job_id = $1",
    )
    .bind(enqueued.job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        completed,
        ("dead".to_owned(), 2, 0, 2, Some("resource_busy".to_owned()),)
    );
    assert!(
        queue
            .claim_many("worker", &kinds, Duration::from_secs(30), 1)
            .await?
            .is_empty()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn retryable_failure_requeues_then_exhausts_attempts()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    let enqueued = queue.enqueue(&job("retry").max_attempts(2)).await?;
    let kinds = vec!["retry".to_owned()];
    let failure = JobFailure::retryable("temporary", "try again");

    let first = queue
        .claim_many("worker", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("first claim");
    assert_eq!(
        queue
            .fail(
                &first,
                &failure,
                AttemptOutcome::RetryableError,
                Duration::ZERO,
            )
            .await?,
        FailureTransition::Retrying
    );
    let second = queue
        .claim_many("worker", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("second claim");
    assert_eq!(
        queue
            .fail(
                &second,
                &failure,
                AttemptOutcome::RetryableError,
                Duration::ZERO,
            )
            .await?,
        FailureTransition::Dead
    );

    let row: (String, i32, i32, i64) = sqlx::query_as(
        "SELECT status, attempt_count, failure_count, \
                (SELECT count(*) FROM background_job_attempts WHERE job_id = $1) \
         FROM background_jobs WHERE job_id = $1",
    )
    .bind(enqueued.job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(row, ("dead".to_owned(), 2, 2, 2));
    assert!(
        queue
            .claim_many("worker", &kinds, Duration::from_secs(30), 1)
            .await?
            .is_empty()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn permanent_failure_does_not_retry() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    let enqueued = queue.enqueue(&job("permanent")).await?;
    let kinds = vec!["permanent".to_owned()];
    let claim = queue
        .claim_many("worker", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("claim");
    assert_eq!(
        queue
            .fail(
                &claim,
                &JobFailure::permanent("invalid", "invalid job"),
                AttemptOutcome::PermanentError,
                Duration::ZERO,
            )
            .await?,
        FailureTransition::Dead
    );
    let row: (String, i32, i32) = sqlx::query_as(
        "SELECT status, attempt_count, failure_count FROM background_jobs WHERE job_id = $1",
    )
    .bind(enqueued.job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(row, ("dead".to_owned(), 1, 1));
    assert!(
        queue
            .claim_many("worker", &kinds, Duration::from_secs(30), 1)
            .await?
            .is_empty()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn expired_lease_is_recovered_and_fences_the_old_claim()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    queue.enqueue(&job("lease")).await?;
    let kinds = vec!["lease".to_owned()];
    let stale = queue
        .claim_many("stale", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("stale claim");
    sqlx::query(
        "UPDATE background_jobs SET lease_until = now() - interval '1 second' \
         WHERE job_id = $1",
    )
    .bind(stale.job_id)
    .execute(&db.pool)
    .await?;
    assert!(!queue.heartbeat(&stale, Duration::from_secs(30)).await?);
    assert!(!queue.succeed(&stale).await?);
    assert_eq!(
        queue
            .fail(
                &stale,
                &JobFailure::retryable("stale_claim", "claim lease expired"),
                AttemptOutcome::RetryableError,
                Duration::ZERO,
            )
            .await?,
        FailureTransition::ClaimLost
    );
    assert_eq!(queue.recover_expired(10).await?.retried, 1);
    let current = queue
        .claim_many("current", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("current claim");

    assert!(!queue.succeed(&stale).await?);
    assert!(queue.succeed(&current).await?);
    let outcomes: Vec<String> = sqlx::query_scalar(
        "SELECT outcome FROM background_job_attempts \
         WHERE job_id = $1 ORDER BY attempt_number",
    )
    .bind(stale.job_id)
    .fetch_all(&db.pool)
    .await?;
    assert_eq!(outcomes, vec!["lease_expired", "succeeded"]);
    let failure_count: i32 =
        sqlx::query_scalar("SELECT failure_count FROM background_jobs WHERE job_id = $1")
            .bind(stale.job_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(failure_count, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn reconciler_advisory_lock_has_one_database_owner() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let mut first = db.pool.begin().await?;
    let mut second = db.pool.begin().await?;

    assert!(
        sqlx::query_scalar::<_, bool>("SELECT try_lock_background_job_reconciler()")
            .fetch_one(&mut *first)
            .await?
    );
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT try_lock_background_job_reconciler()")
            .fetch_one(&mut *second)
            .await?
    );

    first.commit().await?;
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT try_lock_background_job_reconciler()")
            .fetch_one(&mut *second)
            .await?
    );
    second.commit().await?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_reconcilers_do_not_duplicate_lease_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    let enqueued = queue.enqueue(&job("reconciler-race")).await?;
    let kinds = vec!["reconciler-race".to_owned()];
    let claim = queue
        .claim_many("stale-worker", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("claim");
    sqlx::query(
        "UPDATE background_jobs SET lease_until = now() - interval '1 second' \
         WHERE job_id = $1",
    )
    .bind(claim.job_id)
    .execute(&db.pool)
    .await?;

    let config = QueueReconcilerConfig {
        recovery_interval: Duration::from_millis(20),
        retention: Duration::from_secs(90 * 24 * 60 * 60),
        maintenance_interval: Duration::from_secs(60),
    };
    let left = QueueReconciler::new(queue.clone(), config.clone())?;
    let right = QueueReconciler::new(queue, config)?;
    let shutdown = CancellationToken::new();
    let left_shutdown = shutdown.clone();
    let right_shutdown = shutdown.clone();
    let left_task = tokio::spawn(async move { left.run(left_shutdown).await });
    let right_task = tokio::spawn(async move { right.run(right_shutdown).await });

    wait_for_status(&db.pool, enqueued.job_id, "queued").await?;
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::try_join!(left_task, right_task)
    })
    .await
    .map_err(|_| std::io::Error::other("reconcilers did not stop"))??;

    let row: (String, i32, i64) = sqlx::query_as(
        "SELECT job.status, job.attempt_count, count(attempt.job_id) \
         FROM background_jobs job \
         LEFT JOIN background_job_attempts attempt \
           ON attempt.job_id = job.job_id AND attempt.outcome = 'lease_expired' \
         WHERE job.job_id = $1 \
         GROUP BY job.job_id",
    )
    .bind(enqueued.job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(row, ("queued".to_owned(), 1, 1));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn reconciler_drains_terminal_history_larger_than_one_batch()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO background_jobs ( \
             job_kind, payload, status, attempt_count, max_attempts, completed_at \
         ) \
         SELECT 'purge-backlog', '{}'::jsonb, 'succeeded', 1, 1, \
                now() - interval '2 days' \
         FROM generate_series(1, 1001)",
    )
    .execute(&db.pool)
    .await?;

    let queue = JobQueue::new(db.pool.clone());
    let reconciler = QueueReconciler::new(
        queue,
        QueueReconcilerConfig {
            recovery_interval: Duration::from_secs(60),
            retention: Duration::from_secs(24 * 60 * 60),
            maintenance_interval: Duration::from_millis(20),
        },
    )?;
    let shutdown = CancellationToken::new();
    let run_shutdown = shutdown.clone();
    let reconciler_task = tokio::spawn(async move { reconciler.run(run_shutdown).await });

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let remaining: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM background_jobs WHERE job_kind = 'purge-backlog'",
            )
            .fetch_one(&db.pool)
            .await?;
            if remaining == 0 {
                return Ok::<(), sqlx::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("reconciler did not drain terminal history"))??;
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), reconciler_task)
        .await
        .map_err(|_| std::io::Error::other("reconciler did not stop"))??;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn delayed_job_is_not_claimed_early() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    queue
        .enqueue(&job("delayed").available_at(chrono::Utc::now() + chrono::Duration::hours(1)))
        .await?;
    let kinds = vec!["delayed".to_owned()];

    assert!(
        queue
            .claim_many("worker", &kinds, Duration::from_secs(30), 1)
            .await?
            .is_empty()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn expired_final_lease_moves_the_job_to_dead() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    let enqueued = queue.enqueue(&job("lease-dead").max_attempts(1)).await?;
    let kinds = vec!["lease-dead".to_owned()];
    let claim = queue
        .claim_many("worker", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("claim");
    sqlx::query(
        "UPDATE background_jobs SET lease_until = now() - interval '1 second' \
         WHERE job_id = $1",
    )
    .bind(claim.job_id)
    .execute(&db.pool)
    .await?;

    let summary = queue.recover_expired(10).await?;
    assert_eq!(summary.retried, 0);
    assert_eq!(summary.dead, 1);
    let row: (String, i32, Option<String>) = sqlx::query_as(
        "SELECT job.status, job.failure_count, attempt.outcome \
         FROM background_jobs job \
         JOIN background_job_attempts attempt ON attempt.job_id = job.job_id \
         WHERE job.job_id = $1",
    )
    .bind(enqueued.job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        row,
        ("dead".to_owned(), 1, Some("lease_expired".to_owned()))
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn consumer_wake_delay_ignores_expired_running_jobs() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    queue.enqueue(&job("expired-running")).await?;
    let kinds = vec!["expired-running".to_owned()];
    let claim = queue
        .claim_many("worker", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("claim");
    sqlx::query(
        "UPDATE background_jobs SET lease_until = now() - interval '1 second' \
         WHERE job_id = $1",
    )
    .bind(claim.job_id)
    .execute(&db.pool)
    .await?;

    let maximum = Duration::from_secs(300);
    assert_eq!(queue.next_wake_delay(&kinds, maximum).await?, maximum);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn consumer_wake_delay_uses_database_time_for_delayed_jobs()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT enqueue_background_job( \
             'db-clock-delayed', '{}'::jsonb, now() + interval '10 minutes', 8 \
         )",
    )
    .fetch_one(&db.pool)
    .await?;
    let kinds = vec!["db-clock-delayed".to_owned()];

    let delay = JobQueue::new(db.pool.clone())
        .next_wake_delay(&kinds, Duration::from_secs(15 * 60))
        .await?;

    assert!((Duration::from_secs(8 * 60)..=Duration::from_secs(10 * 60)).contains(&delay));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn operational_snapshot_does_not_scan_succeeded_history()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    queue.enqueue(&job("snapshot")).await?;
    queue.enqueue(&job("snapshot")).await?;
    let kinds = vec!["snapshot".to_owned()];
    let claim = queue
        .claim_many("worker", &kinds, Duration::from_secs(30), 1)
        .await?
        .into_iter()
        .next()
        .expect("claim");
    assert!(queue.succeed(&claim).await?);

    let snapshot = queue.snapshot(&kinds).await?;
    assert_eq!(snapshot.states.len(), 1);
    let state = snapshot.states.first().expect("state");
    assert_eq!(state.state, "ready");
    assert_eq!(state.count, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn retention_purge_removes_only_old_terminal_jobs() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    let old = queue.enqueue(&job("retention")).await?;
    let current = queue.enqueue(&job("retention")).await?;
    let kinds = vec!["retention".to_owned()];
    for claim in queue
        .claim_many("worker", &kinds, Duration::from_secs(30), 2)
        .await?
    {
        assert!(queue.succeed(&claim).await?);
    }
    sqlx::query(
        "UPDATE background_jobs \
         SET completed_at = now() - interval '91 days' \
         WHERE job_id = $1",
    )
    .bind(old.job_id)
    .execute(&db.pool)
    .await?;

    assert_eq!(
        queue
            .purge_completed(Duration::from_secs(90 * 24 * 60 * 60), 10)
            .await?,
        1
    );
    let remaining: Vec<uuid::Uuid> =
        sqlx::query_scalar("SELECT job_id FROM background_jobs WHERE job_kind = 'retention'")
            .fetch_all(&db.pool)
            .await?;
    assert_eq!(remaining, vec![current.job_id]);

    db.cleanup().await;
    Ok(())
}
