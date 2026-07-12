//! Background reconciliation for Space usage counters.

use std::future::Future;
use std::time::{Duration, Instant};

use notegate_db::{PgPool, SpaceUsageRepo, UsageReconcileExecution};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::periodic_worker;

const RECONCILE_INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn(pool: PgPool, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(event = "usage_reconcile_worker.started");
        let drain_shutdown = shutdown.clone();
        periodic_worker::run(RECONCILE_INTERVAL, shutdown, || {
            let pool = pool.clone();
            let shutdown = drain_shutdown.clone();
            async move { execute_once(&pool, &shutdown).await }
        })
        .await;
        tracing::info!(event = "usage_reconcile_worker.stopped");
    })
}

/// Drain every ready job (e.g. after a bulk operator enqueue), then yield
/// until the next tick. Deferred and failed jobs leave the ready set, so the
/// loop always terminates.
async fn execute_once(pool: &PgPool, shutdown: &CancellationToken) {
    if shutdown.is_cancelled() {
        return;
    }

    let repo = SpaceUsageRepo::new(pool.clone());
    match repo.try_delete_expired_executions().await {
        Ok(true) => {}
        Ok(false) => {
            tracing::debug!(
                event = "usage_reconcile_worker.skipped",
                reason = "worker_lock_held"
            );
            return;
        }
        Err(error) => {
            tracing::error!(event = "usage_reconcile_worker.retention_failed", %error);
        }
    }

    drain_ready_jobs(shutdown, || execute_next(&repo)).await;
}

async fn drain_ready_jobs<F, Fut>(shutdown: &CancellationToken, mut execute_next: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    while !shutdown.is_cancelled() && execute_next().await {}
}

async fn execute_next(repo: &SpaceUsageRepo) -> bool {
    let started = Instant::now();
    match repo.execute_next_reconciliation().await {
        Ok(UsageReconcileExecution::WorkerLockHeld) => {
            tracing::debug!(
                event = "usage_reconcile_worker.skipped",
                reason = "worker_lock_held"
            );
            false
        }
        Ok(UsageReconcileExecution::Idle) => {
            tracing::debug!(event = "usage_reconcile_worker.idle");
            false
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
            true
        }
        Ok(UsageReconcileExecution::Cancelled { job_id, space_id }) => {
            tracing::info!(
                event = "usage_reconcile_worker.execution",
                outcome = "cancelled",
                %job_id,
                %space_id,
                duration_ms = started.elapsed().as_millis(),
            );
            true
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
            true
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
            true
        }
        Err(error) => {
            tracing::error!(
                event = "usage_reconcile_worker.failed",
                duration_ms = started.elapsed().as_millis(),
                %error,
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn shutdown_stops_the_drain_between_jobs() {
        let shutdown = CancellationToken::new();
        let runs = Arc::new(AtomicUsize::new(0));

        drain_ready_jobs(&shutdown, || {
            let shutdown = shutdown.clone();
            let runs = runs.clone();
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                shutdown.cancel();
                true
            }
        })
        .await;

        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }
}
