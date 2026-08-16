use std::collections::HashSet;
use std::time::{Duration, Instant};

use notegate_jobs::{JobQueue, RecoverySummary};
use notegate_reconciliation::{
    Reconciler, ReconciliationContext, ReconciliationDirective, ReconciliationError,
    ReconciliationFailure, ReconciliationFuture, ReconciliationSchedule,
};

const BACKLOG_CONTINUATION_DELAY: Duration = Duration::from_secs(1);
const LEASE_RECOVERY_INTERVAL: Duration = Duration::from_secs(60);
const LEASE_RECOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const LEASE_RECOVERY_BATCH_SIZE: usize = 256;
const LEASE_RECOVERY_PASS_BUDGET: Duration = Duration::from_secs(5);
const UNREGISTERED_JOB_KIND: &str = "unregistered";

const HISTORY_RETENTION_INTERVAL: Duration = Duration::from_secs(60 * 60);
const HISTORY_RETENTION_TIMEOUT: Duration = Duration::from_secs(30);
const HISTORY_RETENTION: Duration = Duration::from_secs(90 * 24 * 60 * 60);
const HISTORY_RETENTION_BATCH_SIZE: usize = 1_000;
const HISTORY_RETENTION_PASS_BUDGET: Duration = Duration::from_secs(5);

pub(super) struct LeaseRecoveryReconciler {
    queue: JobQueue,
    registered_job_kinds: HashSet<String>,
}

impl LeaseRecoveryReconciler {
    pub(super) fn new(queue: JobQueue, registered_job_kinds: &[String]) -> Self {
        Self {
            queue,
            registered_job_kinds: registered_job_kinds.iter().cloned().collect(),
        }
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
            let directive = loop {
                let summary = self
                    .queue
                    .recover_expired(LEASE_RECOVERY_BATCH_SIZE)
                    .await
                    .map_err(|error| Box::new(error) as ReconciliationFailure)?;
                record_lease_transitions(&summary, &self.registered_job_kinds);
                total_retried = total_retried.saturating_add(summary.retried);
                total_dead = total_dead.saturating_add(summary.dead);
                let processed = summary.retried.saturating_add(summary.dead);
                match batch_action(
                    processed,
                    LEASE_RECOVERY_BATCH_SIZE,
                    started.elapsed(),
                    LEASE_RECOVERY_PASS_BUDGET,
                ) {
                    BatchAction::Complete => break ReconciliationDirective::Complete,
                    BatchAction::ContinuePass => tokio::task::yield_now().await,
                    BatchAction::ContinueSoon => {
                        break ReconciliationDirective::ContinueAfter(BACKLOG_CONTINUATION_DELAY);
                    }
                }
            };
            if total_retried > 0 || total_dead > 0 {
                tracing::info!(
                    event = "background_jobs.leases_recovered",
                    retried = total_retried,
                    dead = total_dead,
                );
            }
            Ok(directive)
        })
    }
}

fn record_lease_transitions(summary: &RecoverySummary, registered_job_kinds: &HashSet<String>) {
    for (kind, transitions) in &summary.by_kind {
        let metric_kind = if registered_job_kinds.contains(kind) {
            kind.as_str()
        } else {
            UNREGISTERED_JOB_KIND
        };
        metrics::counter!(
            "notegate_background_job_transitions",
            "kind" => metric_kind.to_owned(),
            "transition" => "lease_retry"
        )
        .increment(transitions.retried);
        metrics::counter!(
            "notegate_background_job_transitions",
            "kind" => metric_kind.to_owned(),
            "transition" => "lease_dead"
        )
        .increment(transitions.dead);
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
            let directive = loop {
                let deleted = self
                    .queue
                    .purge_completed(HISTORY_RETENTION, HISTORY_RETENTION_BATCH_SIZE)
                    .await
                    .map_err(|error| Box::new(error) as ReconciliationFailure)?;
                total_deleted = total_deleted.saturating_add(deleted);
                match batch_action(
                    deleted,
                    HISTORY_RETENTION_BATCH_SIZE,
                    started.elapsed(),
                    HISTORY_RETENTION_PASS_BUDGET,
                ) {
                    BatchAction::Complete => break ReconciliationDirective::Complete,
                    BatchAction::ContinuePass => tokio::task::yield_now().await,
                    BatchAction::ContinueSoon => {
                        break ReconciliationDirective::ContinueAfter(BACKLOG_CONTINUATION_DELAY);
                    }
                }
            };
            if total_deleted > 0 {
                tracing::info!(
                    event = "background_jobs.history_purged",
                    deleted = total_deleted,
                );
            }
            Ok(directive)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatchAction {
    Complete,
    ContinuePass,
    ContinueSoon,
}

fn batch_action(
    processed: u64,
    batch_size: usize,
    elapsed: Duration,
    budget: Duration,
) -> BatchAction {
    if processed < batch_size as u64 {
        BatchAction::Complete
    } else if elapsed >= budget {
        BatchAction::ContinueSoon
    } else {
        BatchAction::ContinuePass
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use metrics_exporter_prometheus::PrometheusBuilder;
    use notegate_jobs::RecoveryKindSummary;

    use super::*;

    #[test]
    fn lease_transition_metrics_are_scoped_by_job_kind() {
        let recorder = PrometheusBuilder::new()
            .with_recommended_naming(true)
            .build_recorder();
        let handle = recorder.handle();
        let summary = RecoverySummary {
            retried: 5,
            dead: 5,
            by_kind: BTreeMap::from([
                (
                    "graph-link".to_owned(),
                    RecoveryKindSummary {
                        retried: 2,
                        dead: 0,
                    },
                ),
                (
                    "space-usage".to_owned(),
                    RecoveryKindSummary {
                        retried: 0,
                        dead: 1,
                    },
                ),
                (
                    "legacy-job".to_owned(),
                    RecoveryKindSummary {
                        retried: 3,
                        dead: 4,
                    },
                ),
            ]),
        };
        let registered_job_kinds =
            HashSet::from(["graph-link".to_owned(), "space-usage".to_owned()]);

        metrics::with_local_recorder(&recorder, || {
            record_lease_transitions(&summary, &registered_job_kinds)
        });

        let body = handle.render();
        assert!(body.contains(
            "notegate_background_job_transitions_total{kind=\"graph-link\",transition=\"lease_retry\"} 2"
        ));
        assert!(body.contains(
            "notegate_background_job_transitions_total{kind=\"space-usage\",transition=\"lease_dead\"} 1"
        ));
        assert!(body.contains(
            "notegate_background_job_transitions_total{kind=\"unregistered\",transition=\"lease_retry\"} 3"
        ));
        assert!(!body.contains("kind=\"legacy-job\""));
    }

    #[test]
    fn retention_stops_when_the_batch_is_not_full() {
        assert_eq!(
            batch_action(
                HISTORY_RETENTION_BATCH_SIZE as u64 - 1,
                HISTORY_RETENTION_BATCH_SIZE,
                Duration::ZERO,
                HISTORY_RETENTION_PASS_BUDGET,
            ),
            BatchAction::Complete
        );
    }

    #[test]
    fn retention_continues_full_batches_within_the_budget() {
        assert_eq!(
            batch_action(
                HISTORY_RETENTION_BATCH_SIZE as u64,
                HISTORY_RETENTION_BATCH_SIZE,
                Duration::ZERO,
                HISTORY_RETENTION_PASS_BUDGET,
            ),
            BatchAction::ContinuePass
        );
    }

    #[test]
    fn retention_continues_soon_after_the_pass_budget() {
        assert_eq!(
            batch_action(
                HISTORY_RETENTION_BATCH_SIZE as u64,
                HISTORY_RETENTION_BATCH_SIZE,
                HISTORY_RETENTION_PASS_BUDGET,
                HISTORY_RETENTION_PASS_BUDGET,
            ),
            BatchAction::ContinueSoon
        );
    }

    #[test]
    fn recovery_continues_full_batches_within_the_budget() {
        assert_eq!(
            batch_action(
                LEASE_RECOVERY_BATCH_SIZE as u64,
                LEASE_RECOVERY_BATCH_SIZE,
                Duration::ZERO,
                LEASE_RECOVERY_PASS_BUDGET,
            ),
            BatchAction::ContinuePass
        );
    }
}
