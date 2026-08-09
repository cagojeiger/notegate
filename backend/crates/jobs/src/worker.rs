use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::FutureExt as _;
use tokio::task::JoinError;
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::queue::{FailureTransition, validate_job_kind};
use crate::schedule::{Jitter, between, job_entropy};
use crate::{
    AttemptOutcome, ClaimedJob, JobDisposition, JobFailure, JobQueue, JobQueueError, JobQueueResult,
};

const LISTENER_RETRY: Duration = Duration::from_secs(10);
const MIN_POLL_DELAY: Duration = Duration::from_millis(25);
const NOTIFY_WAKE_SPREAD: Duration = Duration::from_millis(50);
const SAFETY_POLL_JITTER_PERCENT: u32 = 10;
const LISTENER_RETRY_JITTER_PERCENT: u32 = 20;
const EXPLICIT_RETRY_JITTER_PERCENT: u32 = 20;

pub trait JobHandler: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    fn timeout(&self) -> Duration;
    fn handle<'a>(
        &'a self,
        job: &'a ClaimedJob,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>>;
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub concurrency: usize,
    pub lease: Duration,
    pub safety_poll: Duration,
    pub retry_base: Duration,
    pub retry_max: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            lease: Duration::from_secs(120),
            safety_poll: Duration::from_secs(10 * 60),
            retry_base: Duration::from_secs(5),
            retry_max: Duration::from_secs(15 * 60),
        }
    }
}

pub struct Worker {
    queue: JobQueue,
    handlers: Arc<HashMap<&'static str, Arc<dyn JobHandler>>>,
    job_kinds: Arc<Vec<String>>,
    config: WorkerConfig,
    worker_id: String,
}

impl Worker {
    pub fn new(
        queue: JobQueue,
        handlers: Vec<Arc<dyn JobHandler>>,
        config: WorkerConfig,
        worker_id: impl Into<String>,
    ) -> JobQueueResult<Self> {
        validate_config(&config)?;
        let worker_id = worker_id.into();
        if worker_id.is_empty() || worker_id.len() > 256 {
            return Err(JobQueueError::InvalidConfiguration(
                "worker id must contain between 1 and 256 bytes".to_owned(),
            ));
        }
        let mut registry = HashMap::new();
        for handler in handlers {
            let kind = handler.kind();
            validate_job_kind(kind)?;
            if handler.timeout().is_zero() {
                return Err(JobQueueError::InvalidConfiguration(format!(
                    "background job handler {kind} must have a positive timeout"
                )));
            }
            if registry.insert(kind, handler).is_some() {
                return Err(JobQueueError::InvalidConfiguration(
                    "duplicate background job handler kind".to_owned(),
                ));
            }
        }
        if registry.is_empty() {
            return Err(JobQueueError::InvalidConfiguration(
                "at least one background job handler is required".to_owned(),
            ));
        }
        let mut job_kinds = registry
            .keys()
            .map(|kind| (*kind).to_owned())
            .collect::<Vec<_>>();
        job_kinds.sort_unstable();
        Ok(Self {
            queue,
            handlers: Arc::new(registry),
            job_kinds: Arc::new(job_kinds),
            config,
            worker_id,
        })
    }

    pub async fn run(&self, shutdown: CancellationToken) -> JobQueueResult<()> {
        let mut listener = None;
        let mut tasks = JoinSet::new();
        let mut jitter = Jitter::random();
        while !shutdown.is_cancelled() {
            if listener.is_none() {
                match self.queue.connect_listener().await {
                    Ok(connected) => listener = Some(connected),
                    Err(error) => {
                        tracing::error!(event = "background_jobs.listen_failed", %error);
                    }
                }
            }

            let claim_failed = self.fill_available(&mut tasks, &shutdown).await;
            if shutdown.is_cancelled() {
                break;
            }

            if tasks.len() == self.config.concurrency {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    result = tasks.join_next() => log_task_result(result),
                }
                continue;
            }

            let mut delay = if claim_failed {
                jitter.symmetric(LISTENER_RETRY, LISTENER_RETRY_JITTER_PERCENT)
            } else {
                let safety_poll =
                    jitter.symmetric(self.config.safety_poll, SAFETY_POLL_JITTER_PERCENT);
                match self
                    .queue
                    .next_wake_delay(&self.job_kinds, safety_poll)
                    .await
                {
                    Ok(delay) => poll_delay(delay),
                    Err(error) => {
                        tracing::error!(event = "background_jobs.wake_query_failed", %error);
                        jitter.symmetric(LISTENER_RETRY, LISTENER_RETRY_JITTER_PERCENT)
                    }
                }
            };

            if claim_failed {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    result = tasks.join_next(), if !tasks.is_empty() => log_task_result(result),
                    () = tokio::time::sleep(delay) => {}
                }
                continue;
            }

            if let Some(connected) = listener.as_mut() {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    result = tasks.join_next(), if !tasks.is_empty() => log_task_result(result),
                    notification = connected.recv() => {
                        match notification {
                            Ok(_notification) => {
                                while connected.next_buffered().is_some() {}
                                let spread = jitter.spread(NOTIFY_WAKE_SPREAD);
                                tokio::select! {
                                    () = shutdown.cancelled() => break,
                                    () = tokio::time::sleep(spread) => {}
                                }
                            }
                            Err(error) => {
                                tracing::warn!(event = "background_jobs.listener_disconnected", %error);
                                listener = None;
                            }
                        }
                    }
                    () = tokio::time::sleep(delay) => {}
                }
            } else {
                delay = delay.min(jitter.symmetric(LISTENER_RETRY, LISTENER_RETRY_JITTER_PERCENT));
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    result = tasks.join_next(), if !tasks.is_empty() => log_task_result(result),
                    () = tokio::time::sleep(delay) => {}
                }
            }
        }

        while let Some(result) = tasks.join_next().await {
            log_task_result(Some(result));
        }
        Ok(())
    }

    async fn fill_available(&self, tasks: &mut JoinSet<()>, shutdown: &CancellationToken) -> bool {
        if shutdown.is_cancelled() || tasks.len() >= self.config.concurrency {
            return false;
        }
        let available = self.config.concurrency - tasks.len();
        let claims = match self
            .queue
            .claim_many(
                &self.worker_id,
                &self.job_kinds,
                self.config.lease,
                available,
            )
            .await
        {
            Ok(claims) => claims,
            Err(error) => {
                tracing::error!(event = "background_jobs.claim_failed", %error);
                return true;
            }
        };
        if claims.is_empty() {
            return false;
        }
        for claim in claims {
            let queue = self.queue.clone();
            let handlers = self.handlers.clone();
            let config = self.config.clone();
            let shutdown = shutdown.clone();
            tasks.spawn(async move {
                execute_claim(queue, handlers, config, claim, shutdown).await;
            });
        }
        false
    }
}

fn log_task_result(result: Option<Result<(), JoinError>>) {
    if let Some(Err(error)) = result {
        tracing::error!(event = "background_jobs.task_failed", %error);
    }
}

async fn execute_claim(
    queue: JobQueue,
    handlers: Arc<HashMap<&'static str, Arc<dyn JobHandler>>>,
    config: WorkerConfig,
    claim: ClaimedJob,
    shutdown: CancellationToken,
) {
    let started = Instant::now();
    metrics::gauge!(
        "notegate_background_jobs_in_flight",
        "kind" => claim.kind.clone()
    )
    .increment(1.0);
    let result = match handlers.get(claim.kind.as_str()) {
        Some(handler) => run_handler(&queue, handler.as_ref(), &claim, &config, &shutdown).await,
        None => HandlerOutcome::Failed {
            failure: JobFailure::permanent(
                "unsupported_job_kind",
                format!("no handler is registered for {}", claim.kind),
            ),
            outcome: AttemptOutcome::PermanentError,
        },
    };
    finish_claim(&queue, &claim, &config, result).await;
    metrics::gauge!(
        "notegate_background_jobs_in_flight",
        "kind" => claim.kind.clone()
    )
    .decrement(1.0);
    metrics::histogram!(
        "notegate_background_job_duration",
        "kind" => claim.kind.clone()
    )
    .record(started.elapsed().as_secs_f64());
}

async fn run_handler(
    queue: &JobQueue,
    handler: &dyn JobHandler,
    claim: &ClaimedJob,
    config: &WorkerConfig,
    shutdown: &CancellationToken,
) -> HandlerOutcome {
    let execution = match std::panic::catch_unwind(AssertUnwindSafe(|| handler.handle(claim))) {
        Ok(execution) => execution,
        Err(_panic) => return panicked_handler(),
    };
    let mut timeout = Box::pin(tokio::time::sleep(handler.timeout()));
    let mut execution = Box::pin(AssertUnwindSafe(execution).catch_unwind());
    let mut lease_monitor = Box::pin(monitor_lease(queue, claim, config.lease));

    tokio::select! {
        () = shutdown.cancelled() => HandlerOutcome::Failed {
            failure: JobFailure::retryable_after(
                "worker_shutdown",
                "worker stopped before the job completed",
                Duration::ZERO,
            ),
            outcome: AttemptOutcome::Cancelled,
        },
        () = &mut timeout => HandlerOutcome::Failed {
            failure: JobFailure::retryable("handler_timeout", "job handler timed out"),
            outcome: AttemptOutcome::TimedOut,
        },
        result = &mut execution => {
            match result {
                Ok(Ok(JobDisposition::Complete)) => HandlerOutcome::Succeeded,
                Ok(Ok(JobDisposition::Defer { reason, retry_after })) => {
                    HandlerOutcome::Deferred { reason, retry_after }
                }
                Ok(Err(failure)) => {
                    let outcome = if failure.class == crate::JobFailureClass::Permanent {
                        AttemptOutcome::PermanentError
                    } else {
                        AttemptOutcome::RetryableError
                    };
                    HandlerOutcome::Failed { failure, outcome }
                }
                Err(_panic) => panicked_handler(),
            }
        },
        () = &mut lease_monitor => HandlerOutcome::ClaimLost,
    }
}

fn panicked_handler() -> HandlerOutcome {
    HandlerOutcome::Failed {
        failure: JobFailure::retryable("handler_panic", "job handler panicked"),
        outcome: AttemptOutcome::Panicked,
    }
}

async fn monitor_lease(queue: &JobQueue, claim: &ClaimedJob, lease: Duration) {
    let mut heartbeat = tokio::time::interval(lease / 3);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    heartbeat.tick().await;
    loop {
        heartbeat.tick().await;
        match queue.heartbeat(claim, lease).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::error!(
                    event = "background_jobs.heartbeat_failed",
                    job_kind = claim.kind,
                    job_id = %claim.job_id,
                    %error,
                );
                return;
            }
        }
    }
}

async fn finish_claim(
    queue: &JobQueue,
    claim: &ClaimedJob,
    config: &WorkerConfig,
    outcome: HandlerOutcome,
) {
    match outcome {
        HandlerOutcome::Succeeded => match queue.succeed(claim).await {
            Ok(true) => record_attempt(claim, "succeeded"),
            Ok(false) => record_attempt(claim, "claim_lost"),
            Err(error) => tracing::error!(
                event = "background_jobs.complete_failed",
                job_kind = claim.kind,
                job_id = %claim.job_id,
                %error,
            ),
        },
        HandlerOutcome::Failed { failure, outcome } => {
            let retry_delay = failure_retry_delay(claim, &failure, config);
            match queue.fail(claim, &failure, outcome, retry_delay).await {
                Ok(FailureTransition::Retrying) => record_attempt(claim, "retrying"),
                Ok(FailureTransition::Dead) => record_attempt(claim, "dead"),
                Ok(FailureTransition::ClaimLost) => record_attempt(claim, "claim_lost"),
                Err(error) => tracing::error!(
                    event = "background_jobs.failure_transition_failed",
                    job_kind = claim.kind,
                    job_id = %claim.job_id,
                    failure_code = failure.code,
                    %error,
                ),
            }
        }
        HandlerOutcome::Deferred {
            reason,
            retry_after,
        } => {
            let retry_delay = explicit_retry_delay(claim, retry_after);
            match queue.defer(claim, reason, retry_delay).await {
                Ok(true) => record_attempt(claim, "deferred"),
                Ok(false) => record_attempt(claim, "claim_lost"),
                Err(error) => tracing::error!(
                    event = "background_jobs.defer_transition_failed",
                    job_kind = claim.kind,
                    job_id = %claim.job_id,
                    defer_reason = reason,
                    %error,
                ),
            }
        }
        HandlerOutcome::ClaimLost => record_attempt(claim, "claim_lost"),
    }
}

fn retry_delay(job_id: Uuid, attempt: i32, base: Duration, maximum: Duration) -> Duration {
    let exponent = u32::try_from(attempt.saturating_sub(1))
        .unwrap_or(0)
        .min(20);
    let multiplier = 1u32.checked_shl(exponent).unwrap_or(u32::MAX);
    let uncapped = base.checked_mul(multiplier).unwrap_or(maximum).min(maximum);
    between(uncapped, 90, 110, job_entropy(job_id, attempt)).min(maximum)
}

fn failure_retry_delay(
    claim: &ClaimedJob,
    failure: &JobFailure,
    config: &WorkerConfig,
) -> Duration {
    failure.retry_after.map_or_else(
        || {
            retry_delay(
                claim.job_id,
                claim.failure_count.saturating_add(1),
                config.retry_base,
                config.retry_max,
            )
        },
        |delay| explicit_retry_delay(claim, delay),
    )
}

fn explicit_retry_delay(claim: &ClaimedJob, delay: Duration) -> Duration {
    between(
        delay,
        100,
        100 + EXPLICIT_RETRY_JITTER_PERCENT,
        job_entropy(claim.job_id, claim.attempt),
    )
}

fn poll_delay(delay: Duration) -> Duration {
    delay.max(MIN_POLL_DELAY)
}

fn record_attempt(claim: &ClaimedJob, outcome: &'static str) {
    metrics::counter!(
        "notegate_background_job_attempts",
        "kind" => claim.kind.clone(),
        "outcome" => outcome,
    )
    .increment(1);
}

fn validate_config(config: &WorkerConfig) -> JobQueueResult<()> {
    if config.concurrency == 0 || config.concurrency > 64 {
        return Err(JobQueueError::InvalidConfiguration(
            "worker concurrency must be between 1 and 64".to_owned(),
        ));
    }
    if config.lease < Duration::from_secs(3) {
        return Err(JobQueueError::InvalidConfiguration(
            "worker lease must be at least 3 seconds".to_owned(),
        ));
    }
    if config.safety_poll.is_zero()
        || config.retry_base.is_zero()
        || config.retry_max < config.retry_base
    {
        return Err(JobQueueError::InvalidConfiguration(
            "worker durations must be positive and retry_max must cover retry_base".to_owned(),
        ));
    }
    Ok(())
}

enum HandlerOutcome {
    Succeeded,
    Deferred {
        reason: &'static str,
        retry_after: Duration,
    },
    Failed {
        failure: JobFailure,
        outcome: AttemptOutcome,
    },
    ClaimLost,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use std::task::{Context, Poll};

    use sqlx::postgres::PgPoolOptions;

    use super::*;

    struct PanicHandler;

    struct ConstructionPanicHandler;

    struct DeferredHandler;

    struct PanicFuture;

    impl Future for PanicFuture {
        type Output = Result<JobDisposition, JobFailure>;

        fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
            panic!("handler panic")
        }
    }

    impl JobHandler for PanicHandler {
        fn kind(&self) -> &'static str {
            "panic"
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(30)
        }

        fn handle<'a>(
            &'a self,
            _job: &'a ClaimedJob,
        ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
            Box::pin(PanicFuture)
        }
    }

    impl JobHandler for ConstructionPanicHandler {
        fn kind(&self) -> &'static str {
            "construction-panic"
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(30)
        }

        fn handle<'a>(
            &'a self,
            _job: &'a ClaimedJob,
        ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
            panic!("handler construction panic")
        }
    }

    impl JobHandler for DeferredHandler {
        fn kind(&self) -> &'static str {
            "deferred"
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(30)
        }

        fn handle<'a>(
            &'a self,
            _job: &'a ClaimedJob,
        ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
            Box::pin(async {
                Ok(JobDisposition::Defer {
                    reason: "resource_busy",
                    retry_after: Duration::from_secs(5),
                })
            })
        }
    }

    struct PendingHandler {
        timeout: Duration,
    }

    impl JobHandler for PendingHandler {
        fn kind(&self) -> &'static str {
            "pending"
        }

        fn timeout(&self) -> Duration {
            self.timeout
        }

        fn handle<'a>(
            &'a self,
            _job: &'a ClaimedJob,
        ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
            Box::pin(std::future::pending())
        }
    }

    fn disconnected_queue() -> JobQueue {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://notegate:notegate@127.0.0.1:1/notegate")
            .expect("lazy pool");
        JobQueue::new(pool)
    }

    fn claim(kind: &str) -> ClaimedJob {
        ClaimedJob {
            job_id: Uuid::new_v4(),
            kind: kind.to_owned(),
            payload: serde_json::Value::Null,
            attempt: 1,
            failure_count: 0,
            max_attempts: 8,
            claim_token: Uuid::new_v4(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn retry_delay_is_bounded_and_grows() {
        let job_id = Uuid::from_u128(1);
        let base = Duration::from_secs(5);
        let maximum = Duration::from_secs(60);
        let first = retry_delay(job_id, 1, base, maximum);
        let second = retry_delay(job_id, 2, base, maximum);
        let far_later = retry_delay(job_id, 100, base, maximum);

        assert!(first >= Duration::from_millis(4_500));
        assert!(second > first);
        assert!(far_later <= maximum);
    }

    #[test]
    fn explicit_retry_delays_never_run_early() {
        let claim = claim("retry-after");
        let base = Duration::from_secs(100);
        let delay = failure_retry_delay(
            &claim,
            &JobFailure::retryable_after("busy", "try later", base),
            &WorkerConfig::default(),
        );

        assert!((base..=Duration::from_secs(120)).contains(&delay));
    }

    #[test]
    fn polling_has_a_minimum_delay() {
        assert_eq!(poll_delay(Duration::ZERO), MIN_POLL_DELAY);
        assert_eq!(poll_delay(Duration::from_secs(1)), Duration::from_secs(1),);
    }

    #[test]
    fn default_safety_poll_is_ten_minutes() {
        assert_eq!(
            WorkerConfig::default().safety_poll,
            Duration::from_secs(10 * 60),
        );
    }

    #[test]
    fn configuration_rejects_zero_concurrency() {
        let error = validate_config(&WorkerConfig {
            concurrency: 0,
            ..WorkerConfig::default()
        });
        assert!(matches!(error, Err(JobQueueError::InvalidConfiguration(_))));
    }

    #[tokio::test]
    async fn worker_rejects_invalid_handler_kinds() {
        struct InvalidHandler;
        impl JobHandler for InvalidHandler {
            fn kind(&self) -> &'static str {
                ""
            }

            fn timeout(&self) -> Duration {
                Duration::from_secs(1)
            }

            fn handle<'a>(
                &'a self,
                _job: &'a ClaimedJob,
            ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>>
            {
                Box::pin(async { Ok(JobDisposition::Complete) })
            }
        }

        let result = Worker::new(
            disconnected_queue(),
            vec![Arc::new(InvalidHandler)],
            WorkerConfig::default(),
            "worker",
        );
        assert!(matches!(
            result,
            Err(JobQueueError::InvalidConfiguration(_))
        ));
    }

    #[tokio::test]
    async fn worker_rejects_zero_handler_timeouts() {
        let result = Worker::new(
            disconnected_queue(),
            vec![Arc::new(PendingHandler {
                timeout: Duration::ZERO,
            })],
            WorkerConfig::default(),
            "worker",
        );
        assert!(matches!(
            result,
            Err(JobQueueError::InvalidConfiguration(_))
        ));
    }

    #[tokio::test]
    async fn handler_panics_become_retryable_failures() {
        let claim = claim("panic");
        let outcome = run_handler(
            &disconnected_queue(),
            &PanicHandler,
            &claim,
            &WorkerConfig::default(),
            &CancellationToken::new(),
        )
        .await;

        assert!(matches!(
            outcome,
            HandlerOutcome::Failed {
                outcome: AttemptOutcome::Panicked,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn handler_construction_panics_become_retryable_failures() {
        let claim = claim("construction-panic");
        let outcome = run_handler(
            &disconnected_queue(),
            &ConstructionPanicHandler,
            &claim,
            &WorkerConfig::default(),
            &CancellationToken::new(),
        )
        .await;

        assert!(matches!(
            outcome,
            HandlerOutcome::Failed {
                outcome: AttemptOutcome::Panicked,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn handler_deferrals_remain_distinct_from_failures() {
        let claim = claim("deferred");
        let outcome = run_handler(
            &disconnected_queue(),
            &DeferredHandler,
            &claim,
            &WorkerConfig::default(),
            &CancellationToken::new(),
        )
        .await;

        let HandlerOutcome::Deferred {
            reason,
            retry_after,
        } = outcome
        else {
            panic!("expected deferred handler outcome");
        };
        assert_eq!(reason, "resource_busy");
        assert_eq!(retry_after, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn handler_timeouts_become_retryable_failures() {
        let claim = claim("pending");
        let outcome = run_handler(
            &disconnected_queue(),
            &PendingHandler {
                timeout: Duration::from_millis(1),
            },
            &claim,
            &WorkerConfig::default(),
            &CancellationToken::new(),
        )
        .await;

        assert!(matches!(
            outcome,
            HandlerOutcome::Failed {
                outcome: AttemptOutcome::TimedOut,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn shutdown_cancels_in_flight_handlers_for_retry() {
        let shutdown = CancellationToken::new();
        shutdown.cancel();
        let claim = claim("pending");
        let outcome = run_handler(
            &disconnected_queue(),
            &PendingHandler {
                timeout: Duration::from_secs(30),
            },
            &claim,
            &WorkerConfig::default(),
            &shutdown,
        )
        .await;

        assert!(matches!(
            outcome,
            HandlerOutcome::Failed {
                outcome: AttemptOutcome::Cancelled,
                ..
            }
        ));
    }
}
