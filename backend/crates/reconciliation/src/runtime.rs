use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::FutureExt as _;
use futures_util::future::join_all;
use sqlx::{Connection as _, PgConnection, PgPool};
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;
use uuid::Uuid;

use crate::registry::RegisteredReconciler;
use crate::{
    ErasedReconciler, ReconciliationContext, ReconciliationDirective, ReconciliationError,
    ReconciliationRegistry,
};

const LOCK_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_RELEASE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ReconciliationRuntime {
    entries: Vec<RegisteredReconciler>,
    locks: PostgresLockProvider,
}

impl ReconciliationRuntime {
    pub fn new(
        pool: &PgPool,
        registry: ReconciliationRegistry,
    ) -> Result<Self, ReconciliationError> {
        if registry.is_empty() {
            return Err(ReconciliationError::InvalidConfiguration(
                "at least one reconciler must be registered".to_owned(),
            ));
        }
        describe_metrics();
        Ok(Self {
            entries: registry.into_entries(),
            locks: PostgresLockProvider::new(pool),
        })
    }

    pub async fn run(self, shutdown: CancellationToken) {
        run(self.entries, self.locks, shutdown).await;
    }
}

async fn run<L>(entries: Vec<RegisteredReconciler>, locks: L, shutdown: CancellationToken)
where
    L: LockProvider,
{
    let kinds = entries.len();
    tracing::info!(event = "reconciliation.started", kinds);
    join_all(
        entries
            .into_iter()
            .map(|entry| run_lane(entry, locks.clone(), shutdown.clone())),
    )
    .await;
    tracing::info!(event = "reconciliation.stopped");
}

async fn run_lane<L>(entry: RegisteredReconciler, locks: L, shutdown: CancellationToken)
where
    L: LockProvider,
{
    let mut ticker = interval(entry.schedule.interval());
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => return,
            _ = ticker.tick() => {
                let execution = execute_once(&entry, &locks, &shutdown).await;
                if let Some(delay) = execution.next_delay(entry.schedule.interval()) {
                    ticker.reset_after(delay);
                }
                if execution.outcome == RunOutcome::Cancelled {
                    return;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunOutcome {
    Succeeded,
    Failed,
    TimedOut,
    Panicked,
    Cancelled,
    LockHeld,
    LockError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RunExecution {
    outcome: RunOutcome,
    continue_after: Option<Duration>,
}

impl RunExecution {
    const fn finished(outcome: RunOutcome) -> Self {
        Self {
            outcome,
            continue_after: None,
        }
    }

    const fn succeeded(continue_after: Option<Duration>) -> Self {
        Self {
            outcome: RunOutcome::Succeeded,
            continue_after,
        }
    }

    fn next_delay(self, interval: Duration) -> Option<Duration> {
        self.continue_after.map(|delay| delay.min(interval))
    }
}

impl RunOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Panicked => "panicked",
            Self::Cancelled => "cancelled",
            Self::LockHeld => "lock_held",
            Self::LockError => "lock_error",
        }
    }
}

async fn execute_once<L>(
    entry: &RegisteredReconciler,
    locks: &L,
    shutdown: &CancellationToken,
) -> RunExecution
where
    L: LockProvider,
{
    let run_id = Uuid::new_v4();
    let lock_attempt = tokio::select! {
        biased;
        () = shutdown.cancelled() => return RunExecution::finished(RunOutcome::Cancelled),
        result = tokio::time::timeout(
            LOCK_ACQUIRE_TIMEOUT,
            locks.try_acquire(entry.lock_key),
        ) => result,
    };
    let Some(lock) = (match lock_attempt {
        Ok(Ok(lock)) => lock,
        Ok(Err(error)) => {
            tracing::error!(
                event = "reconciliation.lock_failed",
                reconciliation_kind = entry.kind,
                %run_id,
                %error,
            );
            record_run(entry.kind, RunOutcome::LockError, Duration::ZERO);
            return RunExecution::finished(RunOutcome::LockError);
        }
        Err(_elapsed) => {
            tracing::error!(
                event = "reconciliation.lock_timed_out",
                reconciliation_kind = entry.kind,
                %run_id,
                timeout_ms = LOCK_ACQUIRE_TIMEOUT.as_millis(),
            );
            record_run(entry.kind, RunOutcome::LockError, Duration::ZERO);
            return RunExecution::finished(RunOutcome::LockError);
        }
    }) else {
        tracing::debug!(
            event = "reconciliation.skipped",
            reconciliation_kind = entry.kind,
            reason = "lock_held",
        );
        record_run(entry.kind, RunOutcome::LockHeld, Duration::ZERO);
        return RunExecution::finished(RunOutcome::LockHeld);
    };

    let started = Instant::now();
    let active = metrics::gauge!(
        "notegate_reconciliation_active",
        "kind" => entry.kind
    );
    active.increment(1.0);
    tracing::info!(
        event = "reconciliation.run_started",
        reconciliation_kind = entry.kind,
        %run_id,
    );

    let context = ReconciliationContext { run_id };
    let execution = run_reconciler(
        entry.reconciler.as_ref(),
        &context,
        entry.kind,
        entry.schedule.timeout(),
        shutdown,
    )
    .await;
    let outcome = execution.outcome;
    let elapsed = started.elapsed();
    active.decrement(1.0);

    match tokio::time::timeout(LOCK_RELEASE_TIMEOUT, lock.release()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::error!(
            event = "reconciliation.lock_release_failed",
            reconciliation_kind = entry.kind,
            %run_id,
            %error,
        ),
        Err(_elapsed) => tracing::error!(
            event = "reconciliation.lock_release_timed_out",
            reconciliation_kind = entry.kind,
            %run_id,
            timeout_ms = LOCK_RELEASE_TIMEOUT.as_millis(),
        ),
    }

    record_run(entry.kind, outcome, elapsed);
    match outcome {
        RunOutcome::Succeeded => tracing::info!(
            event = "reconciliation.succeeded",
            reconciliation_kind = entry.kind,
            %run_id,
            duration_ms = elapsed.as_millis(),
            continue_after = ?execution.continue_after,
        ),
        RunOutcome::Failed => tracing::error!(
            event = "reconciliation.failed",
            reconciliation_kind = entry.kind,
            %run_id,
            duration_ms = elapsed.as_millis(),
        ),
        RunOutcome::TimedOut => tracing::error!(
            event = "reconciliation.timed_out",
            reconciliation_kind = entry.kind,
            %run_id,
            duration_ms = elapsed.as_millis(),
        ),
        RunOutcome::Panicked => tracing::error!(
            event = "reconciliation.panicked",
            reconciliation_kind = entry.kind,
            %run_id,
            duration_ms = elapsed.as_millis(),
        ),
        RunOutcome::Cancelled => tracing::info!(
            event = "reconciliation.cancelled",
            reconciliation_kind = entry.kind,
            %run_id,
            duration_ms = elapsed.as_millis(),
        ),
        RunOutcome::LockHeld | RunOutcome::LockError => {}
    }
    execution
}

async fn run_reconciler(
    reconciler: &dyn ErasedReconciler,
    context: &ReconciliationContext,
    kind: &'static str,
    timeout: Duration,
    shutdown: &CancellationToken,
) -> RunExecution {
    let future = match std::panic::catch_unwind(AssertUnwindSafe(|| reconciler.reconcile(context)))
    {
        Ok(future) => future,
        Err(_panic) => return RunExecution::finished(RunOutcome::Panicked),
    };
    let span = tracing::info_span!(
        "reconciliation.run",
        reconciliation_kind = kind,
        run_id = %context.run_id(),
    );
    let mut future = Box::pin(AssertUnwindSafe(future).catch_unwind().instrument(span));
    let mut deadline = Box::pin(tokio::time::sleep(timeout));

    tokio::select! {
        biased;
        () = shutdown.cancelled() => RunExecution::finished(RunOutcome::Cancelled),
        () = &mut deadline => RunExecution::finished(RunOutcome::TimedOut),
        result = &mut future => match result {
            Ok(Ok(ReconciliationDirective::Complete)) => RunExecution::succeeded(None),
            Ok(Ok(ReconciliationDirective::ContinueAfter(delay))) if !delay.is_zero() => {
                RunExecution::succeeded(Some(delay))
            }
            Ok(Ok(ReconciliationDirective::ContinueAfter(_delay))) => {
                tracing::error!(
                    event = "reconciliation.invalid_continuation_delay",
                    reconciliation_kind = kind,
                    run_id = %context.run_id(),
                );
                RunExecution::finished(RunOutcome::Failed)
            }
            Ok(Err(error)) => {
                tracing::error!(
                    event = "reconciliation.handler_failed",
                    reconciliation_kind = kind,
                    run_id = %context.run_id(),
                    %error,
                );
                RunExecution::finished(RunOutcome::Failed)
            }
            Err(_panic) => RunExecution::finished(RunOutcome::Panicked),
        },
    }
}

fn record_run(kind: &'static str, outcome: RunOutcome, duration: Duration) {
    metrics::counter!(
        "notegate_reconciliation_runs",
        "kind" => kind,
        "outcome" => outcome.as_str()
    )
    .increment(1);
    if !matches!(outcome, RunOutcome::LockHeld | RunOutcome::LockError) {
        metrics::histogram!(
            "notegate_reconciliation_duration",
            "kind" => kind,
            "outcome" => outcome.as_str()
        )
        .record(duration.as_secs_f64());
    }
    if outcome == RunOutcome::Succeeded {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        metrics::gauge!(
            "notegate_reconciliation_last_success_timestamp_seconds",
            "kind" => kind
        )
        .set(timestamp);
    }
}

fn describe_metrics() {
    metrics::describe_gauge!(
        "notegate_reconciliation_active",
        "Reconciliation handlers currently running in this process"
    );
    metrics::describe_counter!(
        "notegate_reconciliation_runs",
        "Reconciliation runs by bounded kind and outcome"
    );
    metrics::describe_histogram!(
        "notegate_reconciliation_duration",
        metrics::Unit::Seconds,
        "Reconciliation handler duration"
    );
    metrics::describe_gauge!(
        "notegate_reconciliation_last_success_timestamp_seconds",
        metrics::Unit::Seconds,
        "Unix timestamp of the last successful reconciliation run"
    );
}

trait LockProvider: Clone + Send + Sync + 'static {
    type Guard: ReconciliationLock;

    async fn try_acquire(&self, lock_key: i64) -> Result<Option<Self::Guard>, ReconciliationError>;
}

trait ReconciliationLock: Send {
    async fn release(self) -> Result<(), ReconciliationError>;
}

#[derive(Clone)]
struct PostgresLockProvider {
    options: Arc<sqlx::postgres::PgConnectOptions>,
}

impl PostgresLockProvider {
    fn new(pool: &PgPool) -> Self {
        Self {
            options: pool.connect_options(),
        }
    }
}

impl LockProvider for PostgresLockProvider {
    type Guard = PostgresLock;

    async fn try_acquire(&self, lock_key: i64) -> Result<Option<Self::Guard>, ReconciliationError> {
        let mut connection = PgConnection::connect_with(self.options.as_ref()).await?;
        let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(lock_key)
            .fetch_one(&mut connection)
            .await?;
        Ok(acquired.then_some(PostgresLock {
            connection,
            lock_key,
        }))
    }
}

struct PostgresLock {
    connection: PgConnection,
    lock_key: i64,
}

impl ReconciliationLock for PostgresLock {
    async fn release(mut self) -> Result<(), ReconciliationError> {
        let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
            .bind(self.lock_key)
            .fetch_one(&mut self.connection)
            .await?;
        if !released {
            return Err(ReconciliationError::AdvisoryLockNotHeld(self.lock_key));
        }
        self.connection.close().await?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::unwrap_in_result
)]
mod tests {
    use std::collections::HashSet;
    use std::io;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use crate::{
        Reconciler, ReconciliationDirective, ReconciliationFailure, ReconciliationFuture,
        ReconciliationResult, ReconciliationSchedule,
    };

    use super::*;

    #[derive(Clone, Default)]
    struct FakeLockProvider {
        held: Arc<Mutex<HashSet<i64>>>,
        fail_acquire: Arc<AtomicBool>,
    }

    impl LockProvider for FakeLockProvider {
        type Guard = FakeLock;

        async fn try_acquire(
            &self,
            lock_key: i64,
        ) -> Result<Option<Self::Guard>, ReconciliationError> {
            if self.fail_acquire.load(Ordering::SeqCst) {
                return Err(ReconciliationError::InvalidConfiguration(
                    "injected lock failure".to_owned(),
                ));
            }
            let mut held = self.held.lock().unwrap();
            if !held.insert(lock_key) {
                return Ok(None);
            }
            Ok(Some(FakeLock {
                held: self.held.clone(),
                lock_key,
                released: false,
            }))
        }
    }

    struct FakeLock {
        held: Arc<Mutex<HashSet<i64>>>,
        lock_key: i64,
        released: bool,
    }

    impl FakeLock {
        fn unlock(&mut self) {
            if !self.released {
                self.held.lock().unwrap().remove(&self.lock_key);
                self.released = true;
            }
        }
    }

    impl Drop for FakeLock {
        fn drop(&mut self) {
            self.unlock();
        }
    }

    impl ReconciliationLock for FakeLock {
        async fn release(mut self) -> Result<(), ReconciliationError> {
            self.unlock();
            Ok(())
        }
    }

    struct SuccessReconciler {
        calls: Arc<AtomicUsize>,
    }

    impl Reconciler for SuccessReconciler {
        const KIND: &'static str = "test.runtime";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(ReconciliationDirective::Complete)
            })
        }
    }

    struct ContinueOnceReconciler {
        calls: Arc<AtomicUsize>,
        continued: Arc<tokio::sync::Notify>,
    }

    impl Reconciler for ContinueOnceReconciler {
        const KIND: &'static str = "test.continue";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            Box::pin(async move {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    Ok(ReconciliationDirective::ContinueAfter(
                        Duration::from_millis(10),
                    ))
                } else {
                    self.continued.notify_one();
                    Ok(ReconciliationDirective::Complete)
                }
            })
        }
    }

    struct InvalidContinuationReconciler;

    impl Reconciler for InvalidContinuationReconciler {
        const KIND: &'static str = "test.invalid_continuation";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            Box::pin(async { Ok(ReconciliationDirective::ContinueAfter(Duration::ZERO)) })
        }
    }

    struct BlockingReconciler {
        started: Arc<tokio::sync::Notify>,
    }

    impl Reconciler for BlockingReconciler {
        const KIND: &'static str = "test.runtime";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            Box::pin(async move {
                self.started.notify_one();
                std::future::pending().await
            })
        }
    }

    struct FailingReconciler;

    impl Reconciler for FailingReconciler {
        const KIND: &'static str = "test.failure";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            Box::pin(async {
                Err(Box::new(io::Error::other("injected failure")) as ReconciliationFailure)
            })
        }
    }

    struct PendingReconciler;

    impl Reconciler for PendingReconciler {
        const KIND: &'static str = "test.timeout";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            Box::pin(std::future::pending())
        }
    }

    struct SynchronousPanicReconciler;

    impl Reconciler for SynchronousPanicReconciler {
        const KIND: &'static str = "test.sync_panic";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            panic!("injected synchronous panic")
        }
    }

    struct AsynchronousPanicReconciler;

    impl Reconciler for AsynchronousPanicReconciler {
        const KIND: &'static str = "test.async_panic";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            Box::pin(async { panic!("injected asynchronous panic") })
        }
    }

    struct ParallelReconcilerA {
        barrier: Arc<tokio::sync::Barrier>,
    }

    impl Reconciler for ParallelReconcilerA {
        const KIND: &'static str = "test.parallel_a";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            Box::pin(async move {
                self.barrier.wait().await;
                std::future::pending().await
            })
        }
    }

    struct ParallelReconcilerB {
        barrier: Arc<tokio::sync::Barrier>,
    }

    impl Reconciler for ParallelReconcilerB {
        const KIND: &'static str = "test.parallel_b";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            Box::pin(async move {
                self.barrier.wait().await;
                std::future::pending().await
            })
        }
    }

    fn schedule(timeout: Duration) -> ReconciliationSchedule {
        ReconciliationSchedule::new(Duration::from_secs(60), timeout).unwrap()
    }

    fn entry<R: Reconciler>(reconciler: R, timeout: Duration) -> RegisteredReconciler {
        ReconciliationRegistry::new()
            .register(reconciler, schedule(timeout))
            .unwrap()
            .into_entries()
            .pop()
            .unwrap()
    }

    #[test]
    fn schedules_reject_zero_durations() {
        assert!(ReconciliationSchedule::new(Duration::ZERO, Duration::from_secs(1)).is_err());
        assert!(ReconciliationSchedule::new(Duration::from_secs(1), Duration::ZERO).is_err());
    }

    #[tokio::test]
    async fn successful_runs_invoke_the_registered_handler() {
        let calls = Arc::new(AtomicUsize::new(0));
        let entry = entry(
            SuccessReconciler {
                calls: calls.clone(),
            },
            Duration::from_secs(1),
        );

        let execution = execute_once(
            &entry,
            &FakeLockProvider::default(),
            &CancellationToken::new(),
        )
        .await;

        assert_eq!(execution, RunExecution::succeeded(None));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn continuation_runs_before_the_registered_interval() {
        let calls = Arc::new(AtomicUsize::new(0));
        let continued = Arc::new(tokio::sync::Notify::new());
        let entry = entry(
            ContinueOnceReconciler {
                calls: calls.clone(),
                continued: continued.clone(),
            },
            Duration::from_secs(1),
        );
        let shutdown = CancellationToken::new();
        let runtime = tokio::spawn({
            let shutdown = shutdown.clone();
            async move {
                run(vec![entry], FakeLockProvider::default(), shutdown).await;
            }
        });

        tokio::time::timeout(Duration::from_secs(1), continued.notified())
            .await
            .expect("continuation should not wait for the one-minute interval");
        shutdown.cancel();
        runtime.await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_held_kind_is_skipped_without_invoking_the_handler() {
        let locks = FakeLockProvider::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let entry = entry(
            SuccessReconciler {
                calls: calls.clone(),
            },
            Duration::from_secs(1),
        );
        let guard = locks.try_acquire(entry.lock_key).await.unwrap().unwrap();

        let execution = execute_once(&entry, &locks, &CancellationToken::new()).await;

        assert_eq!(execution, RunExecution::finished(RunOutcome::LockHeld));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        guard.release().await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_releases_the_kind_for_another_runner() {
        let locks = FakeLockProvider::default();
        let started = Arc::new(tokio::sync::Notify::new());
        let blocked_entry = entry(
            BlockingReconciler {
                started: started.clone(),
            },
            Duration::from_secs(30),
        );
        let shutdown = CancellationToken::new();
        let running = tokio::spawn({
            let locks = locks.clone();
            let shutdown = shutdown.clone();
            async move { execute_once(&blocked_entry, &locks, &shutdown).await }
        });
        started.notified().await;

        let calls = Arc::new(AtomicUsize::new(0));
        let successor = entry(
            SuccessReconciler {
                calls: calls.clone(),
            },
            Duration::from_secs(1),
        );
        assert_eq!(
            execute_once(&successor, &locks, &CancellationToken::new()).await,
            RunExecution::finished(RunOutcome::LockHeld)
        );

        shutdown.cancel();
        assert_eq!(
            running.await.unwrap(),
            RunExecution::finished(RunOutcome::Cancelled)
        );
        assert_eq!(
            execute_once(&successor, &locks, &CancellationToken::new()).await,
            RunExecution::succeeded(None)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn different_kinds_can_run_concurrently() {
        let locks = FakeLockProvider::default();
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let first_shutdown = CancellationToken::new();
        let second_shutdown = CancellationToken::new();
        let first = tokio::spawn({
            let entry = entry(
                ParallelReconcilerA {
                    barrier: barrier.clone(),
                },
                Duration::from_secs(30),
            );
            let locks = locks.clone();
            let shutdown = first_shutdown.clone();
            async move { execute_once(&entry, &locks, &shutdown).await }
        });
        let second = tokio::spawn({
            let entry = entry(
                ParallelReconcilerB {
                    barrier: barrier.clone(),
                },
                Duration::from_secs(30),
            );
            let locks = locks.clone();
            let shutdown = second_shutdown.clone();
            async move { execute_once(&entry, &locks, &shutdown).await }
        });

        tokio::time::timeout(Duration::from_secs(1), barrier.wait())
            .await
            .expect("both reconciliation kinds should start");
        first_shutdown.cancel();
        second_shutdown.cancel();

        assert_eq!(
            first.await.unwrap(),
            RunExecution::finished(RunOutcome::Cancelled)
        );
        assert_eq!(
            second.await.unwrap(),
            RunExecution::finished(RunOutcome::Cancelled)
        );
    }

    #[tokio::test]
    async fn failures_timeouts_and_panics_are_bounded_outcomes() {
        let locks = FakeLockProvider::default();
        let shutdown = CancellationToken::new();

        assert_eq!(
            execute_once(
                &entry(FailingReconciler, Duration::from_secs(1)),
                &locks,
                &shutdown
            )
            .await,
            RunExecution::finished(RunOutcome::Failed)
        );
        assert_eq!(
            execute_once(
                &entry(PendingReconciler, Duration::from_millis(1)),
                &locks,
                &shutdown
            )
            .await,
            RunExecution::finished(RunOutcome::TimedOut)
        );
        assert_eq!(
            execute_once(
                &entry(SynchronousPanicReconciler, Duration::from_secs(1)),
                &locks,
                &shutdown
            )
            .await,
            RunExecution::finished(RunOutcome::Panicked)
        );
        assert_eq!(
            execute_once(
                &entry(AsynchronousPanicReconciler, Duration::from_secs(1)),
                &locks,
                &shutdown
            )
            .await,
            RunExecution::finished(RunOutcome::Panicked)
        );
        assert_eq!(
            execute_once(
                &entry(InvalidContinuationReconciler, Duration::from_secs(1)),
                &locks,
                &shutdown
            )
            .await,
            RunExecution::finished(RunOutcome::Failed)
        );
    }

    #[tokio::test]
    async fn lock_acquisition_failures_do_not_call_the_handler() {
        let locks = FakeLockProvider::default();
        locks.fail_acquire.store(true, Ordering::SeqCst);
        let calls = Arc::new(AtomicUsize::new(0));
        let entry = entry(
            SuccessReconciler {
                calls: calls.clone(),
            },
            Duration::from_secs(1),
        );

        let execution = execute_once(&entry, &locks, &CancellationToken::new()).await;

        assert_eq!(execution, RunExecution::finished(RunOutcome::LockError));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn result_alias_accepts_success() {
        let result: ReconciliationResult = Ok(ReconciliationDirective::Complete);
        assert!(result.is_ok());
    }

    #[test]
    fn continuation_never_postpones_the_registered_interval() {
        let interval = Duration::from_secs(60);

        assert_eq!(
            RunExecution::succeeded(Some(Duration::from_secs(1))).next_delay(interval),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            RunExecution::succeeded(Some(Duration::from_secs(120))).next_delay(interval),
            Some(interval)
        );
    }
}
