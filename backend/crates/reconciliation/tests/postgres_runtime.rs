#![allow(clippy::expect_used, clippy::unwrap_used, clippy::unwrap_in_result)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use notegate_db::test_support::TestDb;
use notegate_reconciliation::{
    Reconciler, ReconciliationContext, ReconciliationDirective, ReconciliationFuture,
    ReconciliationRegistry, ReconciliationRuntime, ReconciliationSchedule,
};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio_util::sync::CancellationToken;

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
        .max_connections(2)
        .connect_with(db.pool.connect_options().as_ref().clone())
        .await?;
    let starts = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(tokio::sync::Notify::new());
    let first = ReconciliationRuntime::new(
        &app_pool,
        ReconciliationRegistry::new().register(
            BlockingReconciler {
                starts: starts.clone(),
                started: started.clone(),
            },
            schedule(),
        )?,
    )?;
    let second = ReconciliationRuntime::new(
        &app_pool,
        ReconciliationRegistry::new().register(
            BlockingReconciler {
                starts: starts.clone(),
                started: started.clone(),
            },
            schedule(),
        )?,
    )?;
    let first_shutdown = CancellationToken::new();
    let second_shutdown = CancellationToken::new();
    let first_task = tokio::spawn(first.run(first_shutdown.clone()));
    let second_task = tokio::spawn(second.run(second_shutdown.clone()));

    tokio::time::timeout(Duration::from_secs(2), started.notified()).await?;
    tokio::time::sleep(Duration::from_millis(50)).await;
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
