use std::time::Duration;

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::schedule::Jitter;
use crate::{JobQueue, JobQueueError, JobQueueResult};

const RECOVERY_BATCH_SIZE: usize = 256;
const PURGE_BATCH_SIZE: usize = 1_000;
const RECOVERY_INITIAL_SPREAD_MAX: Duration = Duration::from_secs(5);
const BACKLOG_CONTINUATION_DELAY: Duration = Duration::from_secs(1);
const ERROR_RETRY_DELAY: Duration = Duration::from_secs(10);
const MAINTENANCE_PASS_BUDGET: Duration = Duration::from_secs(5);
const POLL_JITTER_PERCENT: u32 = 10;

#[derive(Debug, Clone)]
pub struct QueueReconcilerConfig {
    pub recovery_interval: Duration,
    pub retention: Duration,
    pub maintenance_interval: Duration,
}

impl Default for QueueReconcilerConfig {
    fn default() -> Self {
        Self {
            recovery_interval: Duration::from_secs(60),
            retention: Duration::from_secs(90 * 24 * 60 * 60),
            maintenance_interval: Duration::from_secs(60 * 60),
        }
    }
}

pub struct QueueReconciler {
    queue: JobQueue,
    config: QueueReconcilerConfig,
}

impl QueueReconciler {
    pub fn new(queue: JobQueue, config: QueueReconcilerConfig) -> JobQueueResult<Self> {
        if config.recovery_interval.is_zero()
            || config.retention.is_zero()
            || config.maintenance_interval.is_zero()
        {
            return Err(JobQueueError::InvalidConfiguration(
                "queue reconciler durations must be positive".to_owned(),
            ));
        }
        Ok(Self { queue, config })
    }

    pub async fn run(&self, shutdown: CancellationToken) {
        let mut jitter = Jitter::random();
        let recovery_initial_spread = self
            .config
            .recovery_interval
            .min(RECOVERY_INITIAL_SPREAD_MAX);
        let mut recovery = Box::pin(tokio::time::sleep(jitter.spread(recovery_initial_spread)));
        let mut maintenance = Box::pin(tokio::time::sleep(
            jitter.spread(self.config.maintenance_interval),
        ));

        loop {
            tokio::select! {
                biased;
                () = shutdown.cancelled() => return,
                () = &mut recovery => {
                    let delay = jitter.symmetric(
                        self.recover_expired().await,
                        POLL_JITTER_PERCENT,
                    );
                    recovery.as_mut().reset(
                        Instant::now() + delay,
                    );
                },
                () = &mut maintenance => {
                    let delay = jitter.symmetric(
                        self.purge_completed().await,
                        POLL_JITTER_PERCENT,
                    );
                    maintenance.as_mut().reset(
                        Instant::now() + delay,
                    );
                },
            }
        }
    }

    async fn recover_expired(&self) -> Duration {
        match self.queue.try_recover_expired(RECOVERY_BATCH_SIZE).await {
            Ok(Some(summary)) => {
                metrics::counter!(
                    "notegate_background_job_transitions",
                    "transition" => "lease_retry"
                )
                .increment(summary.retried);
                metrics::counter!(
                    "notegate_background_job_transitions",
                    "transition" => "lease_dead"
                )
                .increment(summary.dead);
                if summary.retried.saturating_add(summary.dead) >= RECOVERY_BATCH_SIZE as u64 {
                    BACKLOG_CONTINUATION_DELAY
                } else {
                    self.config.recovery_interval
                }
            }
            Ok(None) => BACKLOG_CONTINUATION_DELAY,
            Err(error) => {
                tracing::error!(event = "background_jobs.recovery_failed", %error);
                ERROR_RETRY_DELAY
            }
        }
    }

    async fn purge_completed(&self) -> Duration {
        let started = Instant::now();
        let mut total_deleted = 0_u64;
        loop {
            match self
                .queue
                .try_purge_completed(self.config.retention, PURGE_BATCH_SIZE)
                .await
            {
                Ok(Some(deleted)) => {
                    total_deleted = total_deleted.saturating_add(deleted);
                    match purge_action(deleted, started.elapsed()) {
                        PurgeAction::Complete => {
                            log_purge(total_deleted, false);
                            return self.config.maintenance_interval;
                        }
                        PurgeAction::ContinueSoon => {
                            log_purge(total_deleted, true);
                            return BACKLOG_CONTINUATION_DELAY;
                        }
                        PurgeAction::ContinuePass => {}
                    }
                    tokio::task::yield_now().await;
                }
                Ok(None) => {
                    log_purge(total_deleted, true);
                    return BACKLOG_CONTINUATION_DELAY;
                }
                Err(error) => {
                    tracing::error!(event = "background_jobs.purge_failed", %error);
                    log_purge(total_deleted, true);
                    return ERROR_RETRY_DELAY;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PurgeAction {
    Complete,
    ContinuePass,
    ContinueSoon,
}

fn purge_action(deleted: u64, elapsed: Duration) -> PurgeAction {
    if deleted < PURGE_BATCH_SIZE as u64 {
        PurgeAction::Complete
    } else if elapsed >= MAINTENANCE_PASS_BUDGET {
        PurgeAction::ContinueSoon
    } else {
        PurgeAction::ContinuePass
    }
}

fn log_purge(deleted: u64, continuation: bool) {
    if deleted > 0 {
        tracing::info!(event = "background_jobs.purged", deleted, continuation);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    fn disconnected_queue() -> JobQueue {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://notegate:notegate@127.0.0.1:1/notegate")
            .expect("lazy pool");
        JobQueue::new(pool)
    }

    #[tokio::test]
    async fn reconciler_rejects_zero_intervals() {
        let result = QueueReconciler::new(
            disconnected_queue(),
            QueueReconcilerConfig {
                recovery_interval: Duration::ZERO,
                ..QueueReconcilerConfig::default()
            },
        );

        assert!(matches!(
            result,
            Err(JobQueueError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn reconciler_defaults_use_a_one_minute_recovery_poll() {
        assert_eq!(
            QueueReconcilerConfig::default().recovery_interval,
            Duration::from_secs(60),
        );
    }

    #[test]
    fn purge_stops_when_the_batch_is_not_full() {
        assert_eq!(
            purge_action(PURGE_BATCH_SIZE as u64 - 1, Duration::ZERO),
            PurgeAction::Complete,
        );
    }

    #[test]
    fn purge_continues_full_batches_within_the_pass_budget() {
        assert_eq!(
            purge_action(PURGE_BATCH_SIZE as u64, Duration::ZERO),
            PurgeAction::ContinuePass,
        );
    }

    #[test]
    fn purge_defers_remaining_batches_after_the_pass_budget() {
        assert_eq!(
            purge_action(PURGE_BATCH_SIZE as u64, MAINTENANCE_PASS_BUDGET),
            PurgeAction::ContinueSoon,
        );
    }
}
