//! Background hard-purge worker.
//!
//! Every notegate server process may spawn this worker. Actual single-worker
//! execution is guaranteed by `PurgeRepo::run_once`, which uses a Postgres
//! advisory transaction lock shared by all instances connected to the same DB.

use std::time::Duration;

use notegate_db::{PgPool, PurgeRepo};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::periodic_worker;

const PURGE_INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn(pool: PgPool, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(event = "purge_worker.started");
        periodic_worker::run(PURGE_INTERVAL, shutdown, || {
            let pool = pool.clone();
            async move { run_once(&pool).await }
        })
        .await;
        tracing::info!(event = "purge_worker.stopped");
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
                spaces_deleted = run.spaces_deleted,
                nodes_deleted = run.nodes_deleted,
                accounts_anonymized = run.accounts_anonymized,
                api_keys_deleted = run.api_keys_deleted,
                browser_sessions_deleted = run.browser_sessions_deleted,
            );
        }
        Err(error) => {
            tracing::error!(event = "purge_worker.failed", %error);
        }
    }
}
