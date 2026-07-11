//! Background reconciliation for Space usage counters.

use std::time::{Duration, Instant};

use chrono::Utc;
use notegate_db::{PgPool, SpaceUsageRepo, UsageReconcileRun};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::periodic_worker;

const RECONCILE_INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn(pool: PgPool, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(event = "usage_reconcile_worker.started");
        periodic_worker::run(RECONCILE_INTERVAL, shutdown, || {
            let pool = pool.clone();
            async move { run_once(&pool).await }
        })
        .await;
        tracing::info!(event = "usage_reconcile_worker.stopped");
    })
}

async fn run_once(pool: &PgPool) {
    let started = Instant::now();
    match SpaceUsageRepo::new(pool.clone())
        .run_reconciliation_once()
        .await
    {
        Ok(UsageReconcileRun::LockHeld) => {
            tracing::debug!(
                event = "usage_reconcile_worker.skipped",
                reason = "worker_lock_held"
            );
        }
        Ok(UsageReconcileRun::Idle) => {
            tracing::debug!(event = "usage_reconcile_worker.idle");
        }
        Ok(UsageReconcileRun::SpacesBusy {
            oldest_space_id,
            oldest_due_at,
            candidates_checked,
            candidates_deferred,
        }) => {
            tracing::debug!(
                event = "usage_reconcile_worker.skipped",
                reason = "space_gates_busy",
                %oldest_space_id,
                candidates_checked,
                candidates_deferred,
                lag_seconds = Utc::now()
                    .signed_duration_since(oldest_due_at)
                    .num_seconds()
                    .max(0),
            );
        }
        Ok(UsageReconcileRun::Reconciled {
            space_id,
            due_at,
            previous,
            actual,
            next_reconcile_at,
        }) => {
            tracing::info!(
                event = "usage_reconcile_worker.run",
                %space_id,
                changed = previous != actual,
                previous_nodes = previous.live_node_count,
                actual_nodes = actual.live_node_count,
                previous_content_bytes = previous.live_content_bytes,
                actual_content_bytes = actual.live_content_bytes,
                lag_seconds = Utc::now()
                    .signed_duration_since(due_at)
                    .num_seconds()
                    .max(0),
                %next_reconcile_at,
                duration_ms = started.elapsed().as_millis(),
            );
        }
        Err(error) => {
            tracing::error!(
                event = "usage_reconcile_worker.failed",
                duration_ms = started.elapsed().as_millis(),
                %error,
            );
        }
    }
}
