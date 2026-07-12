//! Background reconciliation for Space usage counters.

use std::time::{Duration, Instant};

use notegate_db::{PgPool, SpaceUsageRepo, UsageReconcileExecution};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::periodic_worker;

const RECONCILE_INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn(pool: PgPool, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(event = "usage_reconcile_worker.started");
        periodic_worker::run(RECONCILE_INTERVAL, shutdown, || {
            let pool = pool.clone();
            async move { execute_once(&pool).await }
        })
        .await;
        tracing::info!(event = "usage_reconcile_worker.stopped");
    })
}

/// Drain every ready job (e.g. after a bulk operator enqueue), then yield
/// until the next tick. Deferred and failed jobs leave the ready set, so the
/// loop always terminates.
async fn execute_once(pool: &PgPool) {
    let repo = SpaceUsageRepo::new(pool.clone());
    if let Err(error) = repo.delete_expired_executions().await {
        tracing::error!(event = "usage_reconcile_worker.retention_failed", %error);
    }
    loop {
        let started = Instant::now();
        match repo.execute_next_reconciliation().await {
            Ok(UsageReconcileExecution::WorkerLockHeld) => {
                tracing::debug!(
                    event = "usage_reconcile_worker.skipped",
                    reason = "worker_lock_held"
                );
                return;
            }
            Ok(UsageReconcileExecution::Idle) => {
                tracing::debug!(event = "usage_reconcile_worker.idle");
                return;
            }
            Ok(UsageReconcileExecution::Deferred {
                job_id,
                space_id,
                run_after,
            }) => {
                tracing::debug!(
                    event = "usage_reconcile_worker.execution",
                    outcome = "deferred",
                    %job_id,
                    %space_id,
                    %run_after,
                    duration_ms = started.elapsed().as_millis(),
                );
            }
            Ok(UsageReconcileExecution::Cancelled { job_id, space_id }) => {
                tracing::info!(
                    event = "usage_reconcile_worker.execution",
                    outcome = "cancelled",
                    %job_id,
                    %space_id,
                    duration_ms = started.elapsed().as_millis(),
                );
            }
            Ok(UsageReconcileExecution::Succeeded {
                job_id,
                space_id,
                previous,
                actual,
            }) => {
                tracing::info!(
                    event = "usage_reconcile_worker.execution",
                    outcome = "succeeded",
                    %job_id,
                    %space_id,
                    changed = previous != Some(actual),
                    counter_was_missing = previous.is_none(),
                    previous_nodes = previous.map(|counts| counts.live_node_count),
                    actual_nodes = actual.live_node_count,
                    previous_content_bytes = previous.map(|counts| counts.live_content_bytes),
                    actual_content_bytes = actual.live_content_bytes,
                    duration_ms = started.elapsed().as_millis(),
                );
            }
            Ok(UsageReconcileExecution::Failed {
                job_id,
                space_id,
                run_after,
                error,
            }) => {
                tracing::error!(
                    event = "usage_reconcile_worker.execution",
                    outcome = "failed",
                    %job_id,
                    %space_id,
                    %run_after,
                    duration_ms = started.elapsed().as_millis(),
                    %error,
                );
            }
            Err(error) => {
                tracing::error!(
                    event = "usage_reconcile_worker.failed",
                    duration_ms = started.elapsed().as_millis(),
                    %error,
                );
                return;
            }
        }
    }
}
