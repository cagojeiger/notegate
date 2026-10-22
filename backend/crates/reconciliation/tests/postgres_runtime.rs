#![allow(clippy::expect_used, clippy::unwrap_used, clippy::unwrap_in_result)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use notegate_db::test_support::TestDb;
use notegate_reconciliation::{
    LockProvider, PostgresLock, PostgresLockProvider, Reconciler, ReconciliationContext,
    ReconciliationDirective, ReconciliationError, ReconciliationFuture, ReconciliationRegistry,
    ReconciliationRuntime, ReconciliationSchedule,
};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct ObservedLocks {
    inner: PostgresLockProvider,
    attempts: tokio::sync::mpsc::UnboundedSender<bool>,
}

impl LockProvider for ObservedLocks {
    type Guard = PostgresLock;

    async fn try_acquire(&self, lock_key: i64) -> Result<Option<Self::Guard>, ReconciliationError> {
        let result = self.inner.try_acquire(lock_key).await?;
        self.attempts.send(result.is_some()).unwrap();
        Ok(result)
    }
}

struct BlockingReconciler {
    starts: Arc<AtomicUsize>,
    started: Arc<tokio::sync::Notify>,
}

impl Reconciler for BlockingReconciler {
    const KIND: &'static str = "test.postgres_singleton";

    fn reconcile<'a>(&'a self, _context: &'a ReconciliationContext) -> ReconciliationFuture<'a> {
        Box::pin(async move {
            self.starts.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            std::future::pending().await
        })
    }
}

struct DatabaseReconciler {
    pool: PgPool,
    completed: Arc<tokio::sync::Notify>,
}

impl Reconciler for DatabaseReconciler {
    const KIND: &'static str = "test.postgres_singleton";

    fn reconcile<'a>(&'a self, _context: &'a ReconciliationContext) -> ReconciliationFuture<'a> {
        Box::pin(async move {
            sqlx::query("SELECT 1").execute(&self.pool).await?;
            self.completed.notify_one();
            Ok(ReconciliationDirective::Complete)
        })
    }
}

fn schedule() -> ReconciliationSchedule {
    ReconciliationSchedule::new(Duration::from_secs(60), Duration::from_secs(30)).unwrap()
}

#[tokio::test]
async fn postgres_lock_allows_one_kind_and_releases_on_shutdown()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let app_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(db.pool.connect_options().as_ref().clone())
        .await?;
    let starts = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(tokio::sync::Notify::new());
    let (attempts, mut observed_attempts) = tokio::sync::mpsc::unbounded_channel();
    let locks = ObservedLocks {
        inner: PostgresLockProvider::new(&app_pool),
        attempts,
    };
    let first = ReconciliationRuntime::with_lock_provider(
        ReconciliationRegistry::new().register(
            BlockingReconciler {
                starts: starts.clone(),
                started: started.clone(),
            },
            schedule(),
        )?,
        locks.clone(),
    )?;
    let second = ReconciliationRuntime::with_lock_provider(
        ReconciliationRegistry::new().register(
            BlockingReconciler {
                starts: starts.clone(),
                started: started.clone(),
            },
            schedule(),
        )?,
        locks,
    )?;
    let first_shutdown = CancellationToken::new();
    let second_shutdown = CancellationToken::new();
    let first_task = tokio::spawn(first.run(first_shutdown.clone()));
    let second_task = tokio::spawn(second.run(second_shutdown.clone()));

    tokio::time::timeout(Duration::from_secs(2), started.notified()).await?;
    let (first_attempt, second_attempt) = tokio::time::timeout(Duration::from_secs(2), async {
        let first = observed_attempts.recv().await.unwrap();
        let second = observed_attempts.recv().await.unwrap();
        (first, second)
    })
    .await?;
    assert_ne!(first_attempt, second_attempt);
    assert_eq!(starts.load(Ordering::SeqCst), 1);

    first_shutdown.cancel();
    second_shutdown.cancel();
    first_task.await?;
    second_task.await?;

    let completed = Arc::new(tokio::sync::Notify::new());
    let successor = ReconciliationRuntime::new(
        &app_pool,
        ReconciliationRegistry::new().register(
            DatabaseReconciler {
                pool: app_pool.clone(),
                completed: completed.clone(),
            },
            schedule(),
        )?,
    )?;
    let successor_shutdown = CancellationToken::new();
    let successor_task = tokio::spawn(successor.run(successor_shutdown.clone()));

    tokio::time::timeout(Duration::from_secs(2), completed.notified()).await?;
    successor_shutdown.cancel();
    successor_task.await?;

    app_pool.close().await;
    db.cleanup().await;
    Ok(())
}
