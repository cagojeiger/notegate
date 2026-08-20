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
    JobHandler, JobQueue, JobRegistry, JobSpec, NewJob, Worker, WorkerConfig,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

struct BlockingHandler {
    started: Arc<Notify>,
    release: Arc<Semaphore>,
    runs: Arc<AtomicUsize>,
}

macro_rules! job_spec {
    ($name:ident, $kind:literal) => {
        struct $name;

        impl JobSpec for $name {
            const KIND: &'static str = $kind;
            type Payload = Value;
        }
    };
}

job_spec!(WorkerRuntimeJob, "worker-runtime");
job_spec!(TransactionalJob, "transactional");
job_spec!(SupportedJob, "supported");
job_spec!(OtherJob, "other");
job_spec!(SuccessJob, "success");
job_spec!(DeferJob, "defer");
job_spec!(RetryJob, "retry");
job_spec!(PermanentJob, "permanent");
job_spec!(LeaseJob, "lease");
job_spec!(DelayedJob, "delayed");
job_spec!(LeaseDeadJob, "lease-dead");
job_spec!(ExpiredRunningJob, "expired-running");
job_spec!(SnapshotJob, "snapshot");
job_spec!(SnapshotOtherJob, "snapshot-other");
job_spec!(RetentionJob, "retention");

impl JobHandler<WorkerRuntimeJob> for BlockingHandler {
    fn timeout(&self) -> Duration {
        Duration::from_secs(30)
    }

    fn handle<'a>(
        &'a self,
        _job: &'a ClaimedJob,
        _payload: Value,
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

fn job<J>() -> NewJob<J>
where
    J: JobSpec<Payload = Value>,
{
    NewJob::<J>::new(json!({ "subject_id": uuid::Uuid::new_v4() }))
}

fn kinds<J: JobSpec>() -> Vec<String> {
    vec![J::KIND.to_owned()]
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
    let handler = BlockingHandler {
        started: started.clone(),
        release: release.clone(),
        runs: runs.clone(),
    };
    let handlers = JobRegistry::new().register::<WorkerRuntimeJob>(handler)?;
    let worker = Worker::new(queue.clone(), handlers, worker_config(), "runtime-test")?;
    let enqueued = queue.enqueue(&job::<WorkerRuntimeJob>()).await?;
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
    let handlers = JobRegistry::new().register::<WorkerRuntimeJob>(BlockingHandler {
        started: started.clone(),
        release: release.clone(),
        runs: runs.clone(),
    })?;
    let mut config = worker_config();
    config.concurrency = 2;
    let worker = Worker::new(queue.clone(), handlers, config, "capacity-test")?;
    let first = queue.enqueue(&job::<WorkerRuntimeJob>()).await?;
    let shutdown = CancellationToken::new();
    let run_shutdown = shutdown.clone();
    let worker_task = tokio::spawn(async move { worker.run(run_shutdown).await });

    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .map_err(|_| std::io::Error::other("worker did not start the first job"))?;
    let second = queue.enqueue(&job::<WorkerRuntimeJob>()).await?;
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
    JobQueue::enqueue_in(&mut rollback, &job::<TransactionalJob>()).await?;
    rollback.rollback().await?;
    let after_rollback: i64 =
        sqlx::query_scalar("SELECT count(*) FROM background_jobs WHERE job_kind = 'transactional'")
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(after_rollback, 0);

    let mut commit = db.pool.begin().await?;
    JobQueue::enqueue_in(&mut commit, &job::<TransactionalJob>()).await?;
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
async fn legacy_background_runtime_migration_preserves_queued_work()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup_before(32).await? else {
        return Ok(());
    };
    let (_, space_id, _) = space_with_root(&db.pool, "legacy-runtime-removal").await?;
    sqlx::query("INSERT INTO space_usage_reconcile_jobs (space_id, retry_count) VALUES ($1, 3)")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    db.apply_migration(32).await?;
    db.apply_migration(33).await?;
    db.apply_migration(34).await?;
    db.apply_migration(35).await?;

    let queued: (String, String, i32) = sqlx::query_as(
        "SELECT job_kind, status, attempt_count \
         FROM background_jobs WHERE payload ->> 'space_id' = $1",
    )
    .bind(space_id.to_string())
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        queued,
        ("space_usage_reconcile".to_owned(), "queued".to_owned(), 3)
    );
    let removed: (bool, bool, bool, bool, bool) = sqlx::query_as(
        "SELECT \
             to_regclass('space_usage_reconcile_jobs') IS NULL, \
             to_regclass('space_usage_reconcile_executions') IS NULL, \
             to_regprocedure('mirror_legacy_space_usage_job()') IS NULL, \
             to_regprocedure('try_lock_background_job_reconciler()') IS NULL, \
             to_regprocedure( \
                 'enqueue_background_job(text,jsonb,timestamp with time zone,integer)' \
             ) IS NULL",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(removed, (true, true, true, true, true));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn job_history_migration_backfills_active_jobs() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup_before(33).await? else {
        return Ok(());
    };
    let (owner_account_id, backfilled_space_id, _) =
        space_with_root(&db.pool, "jobs-history-backfill").await?;
    sqlx::query(
        "INSERT INTO background_jobs (job_kind, payload) \
         VALUES ('space_usage_reconcile', jsonb_build_object('space_id', $1))",
    )
    .bind(backfilled_space_id)
    .execute(&db.pool)
    .await?;
    let (_, terminal_space_id, _) = space_with_root(&db.pool, "jobs-history-terminal").await?;
    sqlx::query(
        "INSERT INTO background_jobs (job_kind, payload, status, completed_at) \
         VALUES ('space_usage_reconcile', jsonb_build_object('space_id', $1), \
                 'succeeded', now())",
    )
    .bind(terminal_space_id)
    .execute(&db.pool)
    .await?;
    let missing_space_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO background_jobs (job_kind, payload) \
         VALUES ('space_usage_reconcile', jsonb_build_object('space_id', $1))",
    )
    .bind(missing_space_id)
    .execute(&db.pool)
    .await?;

    db.apply_migration(33).await?;

    let backfilled: (
        String,
        Option<uuid::Uuid>,
        Option<String>,
        Option<uuid::Uuid>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT history_visibility, history_owner_account_id, \
                context_kind, context_id, context_label \
         FROM background_jobs WHERE payload ->> 'space_id' = $1",
    )
    .bind(backfilled_space_id.to_string())
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        backfilled,
        (
            "visible".to_owned(),
            Some(owner_account_id),
            Some("space".to_owned()),
            Some(backfilled_space_id),
            Some("ws-jobs-history-backfill".to_owned())
        )
    );

    let legacy_hidden: Vec<(String, Option<uuid::Uuid>)> = sqlx::query_as(
        "SELECT history_visibility, history_owner_account_id \
         FROM background_jobs \
         WHERE payload ->> 'space_id' IN ($1, $2) \
         ORDER BY payload ->> 'space_id'",
    )
    .bind(terminal_space_id.to_string())
    .bind(missing_space_id.to_string())
    .fetch_all(&db.pool)
    .await?;
    assert_eq!(
        legacy_hidden,
        vec![("hidden".to_owned(), None), ("hidden".to_owned(), None)]
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn link_job_history_migration_backfills_and_preserves_rolling_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup_before(39).await? else {
        return Ok(());
    };
    let (owner_account_id, space_id, _) =
        space_with_root(&db.pool, "link-jobs-history-backfill").await?;
    let job_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO background_jobs (job_kind, payload, status, completed_at) \
         VALUES ('link_graph_project_nodes', jsonb_build_object('space_id', $1), \
                 'succeeded', now()) \
         RETURNING job_id",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;

    db.apply_migration(39).await?;

    let rolling_job_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT enqueue_background_job( \
             'link_graph_project_nodes', jsonb_build_object('space_id', $1), \
             now(), 8, 'hidden', NULL::uuid, NULL::text, NULL::uuid, NULL::text \
         )",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;

    let history: (
        String,
        Option<uuid::Uuid>,
        Option<String>,
        Option<uuid::Uuid>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT history_visibility, history_owner_account_id, \
                context_kind, context_id, context_label \
         FROM background_jobs WHERE job_id = $1",
    )
    .bind(job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        history,
        (
            "visible".to_owned(),
            Some(owner_account_id),
            Some("space".to_owned()),
            Some(space_id),
            Some("ws-link-jobs-history-backfill".to_owned())
        )
    );

    let rolling_history: (
        String,
        Option<uuid::Uuid>,
        Option<uuid::Uuid>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT history_visibility, history_owner_account_id, context_id, context_label \
             FROM background_jobs WHERE job_id = $1",
    )
    .bind(rolling_job_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        rolling_history,
        (
            "visible".to_owned(),
            Some(owner_account_id),
            Some(space_id),
            Some("ws-link-jobs-history-backfill".to_owned())
        )
    );

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
    queue.enqueue(&job::<SupportedJob>()).await?;
    queue.enqueue(&job::<SupportedJob>()).await?;
    queue.enqueue(&job::<OtherJob>()).await?;
    let kinds = kinds::<SupportedJob>();

    let left = queue.clone();
    let right = queue.clone();
    let (left, right) = tokio::join!(
        left.claim_many("left", &kinds, Duration::from_secs(30), 1),
        right.claim_many("right", &kinds, Duration::from_secs(30), 1),
    );
    let left = left?.into_iter().next().expect("left claim");
    let right = right?.into_iter().next().expect("right claim");
    assert_ne!(left.job_id, right.job_id);
    assert_eq!(left.kind, SupportedJob::KIND);
    assert_eq!(right.kind, SupportedJob::KIND);

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
    let enqueued = queue.enqueue(&job::<SuccessJob>()).await?;
    let kinds = kinds::<SuccessJob>();
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
    let enqueued = queue.enqueue(&job::<DeferJob>().max_attempts(2)).await?;
    let kinds = kinds::<DeferJob>();

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
    let enqueued = queue.enqueue(&job::<RetryJob>().max_attempts(2)).await?;
    let kinds = kinds::<RetryJob>();
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
    let enqueued = queue.enqueue(&job::<PermanentJob>()).await?;
    let kinds = kinds::<PermanentJob>();
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
    queue.enqueue(&job::<LeaseJob>()).await?;
    let kinds = kinds::<LeaseJob>();
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
    let recovery = queue.recover_expired(10).await?;
    assert_eq!(recovery.retried, 1);
    let lease = recovery.by_kind.get(LeaseJob::KIND).expect("kind");
    assert_eq!(lease.retried, 1);
    assert_eq!(lease.dead, 0);
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
async fn delayed_job_is_not_claimed_early() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let queue = JobQueue::new(db.pool.clone());
    queue
        .enqueue(&job::<DelayedJob>().available_at(chrono::Utc::now() + chrono::Duration::hours(1)))
        .await?;
    let kinds = kinds::<DelayedJob>();

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
    let enqueued = queue
        .enqueue(&job::<LeaseDeadJob>().max_attempts(1))
        .await?;
    let kinds = kinds::<LeaseDeadJob>();
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
    let lease_dead = summary.by_kind.get(LeaseDeadJob::KIND).expect("kind");
    assert_eq!(lease_dead.retried, 0);
    assert_eq!(lease_dead.dead, 1);
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
    queue.enqueue(&job::<ExpiredRunningJob>()).await?;
    let kinds = kinds::<ExpiredRunningJob>();
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
             'db-clock-delayed', '{}'::jsonb, now() + interval '10 minutes', 8, \
             'hidden', NULL::uuid, NULL::text, NULL::uuid, NULL::text \
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
    queue.enqueue(&job::<SnapshotJob>()).await?;
    queue.enqueue(&job::<SnapshotJob>()).await?;
    queue.enqueue(&job::<SnapshotOtherJob>()).await?;
    let snapshot_kinds = vec![
        SnapshotJob::KIND.to_owned(),
        SnapshotOtherJob::KIND.to_owned(),
    ];
    let claim = queue
        .claim_many(
            "worker",
            &kinds::<SnapshotJob>(),
            Duration::from_secs(30),
            1,
        )
        .await?
        .into_iter()
        .next()
        .expect("claim");
    assert!(queue.succeed(&claim).await?);

    let snapshot = queue.snapshot(&snapshot_kinds).await?;
    assert_eq!(snapshot.states.len(), 2);
    assert!(snapshot.states.iter().all(|state| state.state == "ready"));
    assert!(snapshot.states.iter().all(|state| state.count == 1));
    assert_eq!(snapshot.oldest_ready.len(), 2);
    assert!(
        snapshot
            .oldest_ready
            .iter()
            .any(|oldest| oldest.kind == SnapshotJob::KIND)
    );
    assert!(
        snapshot
            .oldest_ready
            .iter()
            .any(|oldest| oldest.kind == SnapshotOtherJob::KIND)
    );

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
    let old = queue.enqueue(&job::<RetentionJob>()).await?;
    let current = queue.enqueue(&job::<RetentionJob>()).await?;
    let kinds = kinds::<RetentionJob>();
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
