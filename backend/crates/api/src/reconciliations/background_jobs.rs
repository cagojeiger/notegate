use std::time::{Duration, Instant};

use notegate_jobs::JobQueue;
use notegate_reconciliation::{
    Reconciler, ReconciliationContext, ReconciliationError, ReconciliationFailure,
    ReconciliationFuture, ReconciliationSchedule,
};

const LEASE_RECOVERY_INTERVAL: Duration = Duration::from_secs(60);
const LEASE_RECOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const LEASE_RECOVERY_BATCH_SIZE: usize = 256;
const LEASE_RECOVERY_PASS_BUDGET: Duration = Duration::from_secs(5);

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
            let started = Instant::now();
            let mut total_retried = 0_u64;
            let mut total_dead = 0_u64;
            loop {
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
                total_retried = total_retried.saturating_add(summary.retried);
                total_dead = total_dead.saturating_add(summary.dead);
                let processed = summary.retried.saturating_add(summary.dead);
                if !continue_batch_pass(
                    processed,
                    LEASE_RECOVERY_BATCH_SIZE,
                    started.elapsed(),
                    LEASE_RECOVERY_PASS_BUDGET,
                ) {
                    break;
                }
                tokio::task::yield_now().await;
            }
            if total_retried > 0 || total_dead > 0 {
                tracing::info!(
                    event = "background_jobs.leases_recovered",
                    retried = total_retried,
                    dead = total_dead,
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
                if !continue_batch_pass(
                    deleted,
                    HISTORY_RETENTION_BATCH_SIZE,
                    started.elapsed(),
                    HISTORY_RETENTION_PASS_BUDGET,
                ) {
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

fn continue_batch_pass(
    processed: u64,
    batch_size: usize,
    elapsed: Duration,
    budget: Duration,
) -> bool {
    processed == batch_size as u64 && elapsed < budget
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_stops_when_the_batch_is_not_full() {
        assert!(!continue_batch_pass(
            HISTORY_RETENTION_BATCH_SIZE as u64 - 1,
            HISTORY_RETENTION_BATCH_SIZE,
            Duration::ZERO,
            HISTORY_RETENTION_PASS_BUDGET,
        ));
    }

    #[test]
    fn retention_continues_full_batches_within_the_budget() {
        assert!(continue_batch_pass(
            HISTORY_RETENTION_BATCH_SIZE as u64,
            HISTORY_RETENTION_BATCH_SIZE,
            Duration::ZERO,
            HISTORY_RETENTION_PASS_BUDGET,
        ));
    }

    #[test]
    fn retention_stops_after_the_pass_budget() {
        assert!(!continue_batch_pass(
            HISTORY_RETENTION_BATCH_SIZE as u64,
            HISTORY_RETENTION_BATCH_SIZE,
            HISTORY_RETENTION_PASS_BUDGET,
            HISTORY_RETENTION_PASS_BUDGET,
        ));
    }

    #[test]
    fn recovery_continues_full_batches_within_the_budget() {
        assert!(continue_batch_pass(
            LEASE_RECOVERY_BATCH_SIZE as u64,
            LEASE_RECOVERY_BATCH_SIZE,
            Duration::ZERO,
            LEASE_RECOVERY_PASS_BUDGET,
        ));
    }
}
