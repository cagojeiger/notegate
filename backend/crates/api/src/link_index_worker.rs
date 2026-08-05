use std::time::Duration;

use notegate_service::link_index::{LinkIndexExecution, LinkIndexService};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::periodic_worker;

const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub fn spawn(service: LinkIndexService, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(event = "link_index_worker.started");
        let drain_shutdown = shutdown.clone();
        periodic_worker::run(POLL_INTERVAL, shutdown, || {
            let service = service.clone();
            let shutdown = drain_shutdown.clone();
            async move { drain_ready(&service, &shutdown).await }
        })
        .await;
        tracing::info!(event = "link_index_worker.stopped");
    })
}

async fn drain_ready(service: &LinkIndexService, shutdown: &CancellationToken) {
    while !shutdown.is_cancelled() {
        match service.execute_next().await {
            Ok(LinkIndexExecution::Idle) => return,
            Ok(LinkIndexExecution::SpaceExpanded { space_id }) => {
                tracing::debug!(event = "link_index_worker.space_expanded", %space_id);
            }
            Ok(LinkIndexExecution::SourceIndexed {
                space_id,
                source_node_id,
                reference_count,
            }) => {
                tracing::debug!(
                    event = "link_index_worker.source_indexed",
                    %space_id,
                    %source_node_id,
                    reference_count,
                );
            }
            Ok(LinkIndexExecution::SourceDiscarded {
                space_id,
                source_node_id,
            }) => {
                tracing::debug!(
                    event = "link_index_worker.source_discarded",
                    %space_id,
                    %source_node_id,
                );
            }
            Ok(LinkIndexExecution::ClaimLost) => {
                tracing::debug!(event = "link_index_worker.claim_lost");
            }
            Ok(LinkIndexExecution::Failed {
                space_id,
                source_node_id,
                error,
            }) => {
                tracing::warn!(
                    event = "link_index_worker.retry_scheduled",
                    %space_id,
                    ?source_node_id,
                    %error,
                );
            }
            Err(error) => {
                tracing::error!(event = "link_index_worker.failed", %error);
                return;
            }
        }
    }
}
