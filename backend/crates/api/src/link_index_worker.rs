//! Background worker for the eventually consistent Markdown link projection.

use std::time::{Duration, Instant};

use notegate_service::link_index::{LinkIndexRun, LinkIndexService};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::periodic_worker;

const LINK_INDEX_INTERVAL: Duration = Duration::from_secs(2);

pub fn spawn(service: LinkIndexService, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(event = "link_index_worker.started");
        let drain_shutdown = shutdown.clone();
        periodic_worker::run(LINK_INDEX_INTERVAL, shutdown, || {
            let service = service.clone();
            let shutdown = drain_shutdown.clone();
            async move { drain_ready_spaces(&service, &shutdown).await }
        })
        .await;
        tracing::info!(event = "link_index_worker.stopped");
    })
}

async fn drain_ready_spaces(service: &LinkIndexService, shutdown: &CancellationToken) {
    while !shutdown.is_cancelled() {
        let started = Instant::now();
        match service.process_next().await {
            Ok(LinkIndexRun::Idle) => return,
            Ok(LinkIndexRun::Incremental { space_id, events }) => {
                tracing::debug!(
                    event = "link_index_worker.run",
                    outcome = "incremental",
                    %space_id,
                    events,
                    duration_ms = started.elapsed().as_millis(),
                );
            }
            Ok(LinkIndexRun::Rebuilt { space_id }) => {
                tracing::info!(
                    event = "link_index_worker.run",
                    outcome = "rebuilt",
                    %space_id,
                    duration_ms = started.elapsed().as_millis(),
                );
            }
            Ok(LinkIndexRun::RebuildQueued { space_id }) => {
                tracing::debug!(
                    event = "link_index_worker.run",
                    outcome = "rebuild_queued",
                    %space_id,
                    duration_ms = started.elapsed().as_millis(),
                );
            }
            Err(error) => {
                tracing::error!(
                    event = "link_index_worker.failed",
                    duration_ms = started.elapsed().as_millis(),
                    %error,
                );
                return;
            }
        }
    }
}
