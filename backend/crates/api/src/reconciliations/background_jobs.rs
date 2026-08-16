use std::time::{Duration, Instant};

use notegate_jobs::JobQueue;
use notegate_reconciliation::{
    Reconciler, ReconciliationContext, ReconciliationError, ReconciliationFailure,
    ReconciliationFuture, ReconciliationSchedule,
};

const LEASE_RECOVERY_INTERVAL: Duration = Duration::from_secs(60);
const LEASE_RECOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const LEASE_RECOVERY_BATCH_SIZE: usize = 256;

const HISTORY_RETENTION_INTERVAL: Duration = Duration::from_secs(60 * 60);
const HISTORY_RETENTION_TIMEOUT: Duration = Duration::from_secs(30);
const HISTORY_RETENTION: Duration = Duration::from_secs(90 * 24 * 60 * 60);
const HISTORY_RETENTION_BATCH_SIZE: usize = 1_000;
const HISTORY_RETENTION_PASS_BUDGET: Duration = Duration::from_secs(5);

pub(super) struct LeaseRecoveryReconciler {
    queue: JobQueue,
}

impl LeaseRecoveryReconciler {
    pub(super) fn new(queue: JobQueue) -> Self {
        Self { queue }
    }

    pub(super) fn schedule() -> Result<ReconciliationSchedule, ReconciliationError> {
        ReconciliationSchedule::new(LEASE_RECOVERY_INTERVAL, LEASE_RECOVERY_TIMEOUT)
    }
}

impl Reconciler for LeaseRecoveryReconciler {
    const KIND: &'static str = "background_jobs.lease_recovery";

    fn reconcile<'a>(&'a self, _context: &'a ReconciliationContext) -> ReconciliationFuture<'a> {
        Box::pin(async move {
            let summary = self
                .queue
                .recover_expired(LEASE_RECOVERY_BATCH_SIZE)
                .await
                .map_err(|error| Box::new(error) as ReconciliationFailure)?;
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
            if summary.retried > 0 || summary.dead > 0 {
                tracing::info!(
                    event = "background_jobs.leases_recovered",
                    retried = summary.retried,
                    dead = summary.dead,
                );
            }
            Ok(())
        })
    }
}

pub(super) struct JobHistoryRetentionReconciler {
    queue: JobQueue,
}

impl JobHistoryRetentionReconciler {
    pub(super) fn new(queue: JobQueue) -> Self {
        Self { queue }
    }

    pub(super) fn schedule() -> Result<ReconciliationSchedule, ReconciliationError> {
        ReconciliationSchedule::new(HISTORY_RETENTION_INTERVAL, HISTORY_RETENTION_TIMEOUT)
    }
}

impl Reconciler for JobHistoryRetentionReconciler {
    const KIND: &'static str = "background_jobs.history_retention";

    fn reconcile<'a>(&'a self, _context: &'a ReconciliationContext) -> ReconciliationFuture<'a> {
        Box::pin(async move {
            let started = Instant::now();
            let mut total_deleted = 0_u64;
            loop {
                let deleted = self
                    .queue
                    .purge_completed(HISTORY_RETENTION, HISTORY_RETENTION_BATCH_SIZE)
                    .await
                    .map_err(|error| Box::new(error) as ReconciliationFailure)?;
                total_deleted = total_deleted.saturating_add(deleted);
                if !continue_retention_pass(deleted, started.elapsed()) {
                    break;
                }
                tokio::task::yield_now().await;
            }
            if total_deleted > 0 {
                tracing::info!(
                    event = "background_jobs.history_purged",
                    deleted = total_deleted,
                );
            }
            Ok(())
        })
    }
}

fn continue_retention_pass(deleted: u64, elapsed: Duration) -> bool {
    deleted == HISTORY_RETENTION_BATCH_SIZE as u64 && elapsed < HISTORY_RETENTION_PASS_BUDGET
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_stops_when_the_batch_is_not_full() {
        assert!(!continue_retention_pass(
            HISTORY_RETENTION_BATCH_SIZE as u64 - 1,
            Duration::ZERO,
        ));
    }

    #[test]
    fn retention_continues_full_batches_within_the_budget() {
        assert!(continue_retention_pass(
            HISTORY_RETENTION_BATCH_SIZE as u64,
            Duration::ZERO,
        ));
    }

    #[test]
    fn retention_stops_after_the_pass_budget() {
        assert!(!continue_retention_pass(
            HISTORY_RETENTION_BATCH_SIZE as u64,
            HISTORY_RETENTION_PASS_BUDGET,
        ));
    }
}
