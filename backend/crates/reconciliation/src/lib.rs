use std::time::Duration;

use notegate_db::PgPool;
use notegate_service::link_index::{LinkIndexExecution, LinkIndexService};
use sqlx::postgres::PgListener;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const NOTIFY_CHANNEL: &str = "notegate_reconciliation";
const LISTENER_RETRY: Duration = Duration::from_secs(5);
const SAFETY_POLL_BASE: Duration = Duration::from_secs(5 * 60);
const SAFETY_POLL_JITTER_SECS: u64 = 60;

pub async fn run(
    pool: &PgPool,
    links: &LinkIndexService,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    while !shutdown.is_cancelled() {
        match listen(pool).await {
            Ok(mut listener) => {
                drain(links, &shutdown).await;
                if wait_for_work(&mut listener, links, &shutdown).await {
                    return Ok(());
                }
            }
            Err(error) => {
                tracing::error!(event = "reconciliation.listen_failed", %error);
            }
        }

        tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            () = tokio::time::sleep(LISTENER_RETRY) => {}
        }
    }
    Ok(())
}

async fn listen(pool: &PgPool) -> Result<PgListener, sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen(NOTIFY_CHANNEL).await?;
    Ok(listener)
}

async fn wait_for_work(
    listener: &mut PgListener,
    links: &LinkIndexService,
    shutdown: &CancellationToken,
) -> bool {
    loop {
        let safety_poll = safety_poll_interval();
        tokio::select! {
            () = shutdown.cancelled() => return true,
            notification = listener.recv() => {
                match notification {
                    Ok(notification) if notification.payload() == "projection" => {
                        drain(links, shutdown).await;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(event = "reconciliation.listener_disconnected", %error);
                        return false;
                    }
                }
            }
            () = tokio::time::sleep(safety_poll) => {
                drain(links, shutdown).await;
            }
        }
    }
}

fn safety_poll_interval() -> Duration {
    let [jitter, ..] = Uuid::new_v4().into_bytes();
    SAFETY_POLL_BASE + Duration::from_secs(u64::from(jitter) % SAFETY_POLL_JITTER_SECS)
}

async fn drain(links: &LinkIndexService, shutdown: &CancellationToken) {
    while !shutdown.is_cancelled() {
        match links.execute_next().await {
            Ok(LinkIndexExecution::Idle) => return,
            Ok(LinkIndexExecution::SpaceExpanded { space_id }) => {
                tracing::debug!(event = "link_index.space_expanded", %space_id);
            }
            Ok(LinkIndexExecution::SourceIndexed {
                space_id,
                source_node_id,
                reference_count,
            }) => {
                tracing::debug!(
                    event = "link_index.source_indexed",
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
                    event = "link_index.source_discarded",
                    %space_id,
                    %source_node_id,
                );
            }
            Ok(LinkIndexExecution::ClaimLost) => {
                tracing::debug!(event = "link_index.claim_lost");
            }
            Ok(LinkIndexExecution::Failed {
                space_id,
                source_node_id,
                error,
            }) => {
                tracing::warn!(
                    event = "link_index.retry_scheduled",
                    %space_id,
                    ?source_node_id,
                    %error,
                );
            }
            Err(error) => {
                tracing::error!(event = "reconciliation.drain_failed", %error);
                return;
            }
        }
    }
}
