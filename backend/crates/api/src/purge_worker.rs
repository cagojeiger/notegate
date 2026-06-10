//! Background hard-purge worker.
//!
//! Every notegate server process may spawn this worker. Actual single-worker
//! execution is guaranteed by `PurgeRepo::run_once`, which uses a Postgres
//! advisory transaction lock shared by all instances connected to the same DB.

use std::time::Duration;

use notegate_db::{PgPool, PurgeRepo};
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;

const PURGE_INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn(pool: PgPool, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(event = "purge_worker.started");
        run_once(&pool).await;

        let mut ticker = interval(PURGE_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await;

        loop {
            tokio::select! {
                () = shutdown.cancelled() => {
                    tracing::info!(event = "purge_worker.stopped");
                    return;
                }
                _ = ticker.tick() => run_once(&pool).await,
            }
        }
    })
}

async fn run_once(pool: &PgPool) {
    match PurgeRepo::new(pool.clone()).run_once().await {
        Ok(run) if !run.lock_acquired => {
            tracing::debug!(event = "purge_worker.skipped", reason = "lock_held");
        }
        Ok(run) => {
            tracing::info!(
                event = "purge_worker.run",
                workspaces_deleted = run.workspaces_deleted,
                nodes_deleted = run.nodes_deleted,
                accounts_anonymized = run.accounts_anonymized,
            );
        }
        Err(error) => {
            tracing::error!(event = "purge_worker.failed", %error);
        }
    }
}
