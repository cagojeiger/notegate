use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use notegate_db::{
    LINK_GRAPH_PROJECT_BATCH_MAX, LinkGraphProjectNodesJob, LinkGraphProjectNodesPayload,
};
use notegate_jobs::{ClaimedJob, JobDisposition, JobFailure, JobHandler};
use notegate_service::link_graph::LinkGraphService;

#[derive(Clone)]
pub(super) struct LinkGraphProjectNodesHandler {
    graph: LinkGraphService,
}

impl LinkGraphProjectNodesHandler {
    pub(super) fn new(graph: LinkGraphService) -> Self {
        Self { graph }
    }
}

impl JobHandler<LinkGraphProjectNodesJob> for LinkGraphProjectNodesHandler {
    fn timeout(&self) -> Duration {
        Duration::from_secs(70)
    }

    fn handle<'a>(
        &'a self,
        job: &'a ClaimedJob,
        payload: LinkGraphProjectNodesPayload,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
        Box::pin(async move {
            if payload.sources.is_empty() || payload.sources.len() > LINK_GRAPH_PROJECT_BATCH_MAX {
                return Err(JobFailure::permanent(
                    "invalid_link_graph_batch",
                    format!(
                        "link graph batch must contain between 1 and {LINK_GRAPH_PROJECT_BATCH_MAX} node ids"
                    ),
                ));
            }
            let result = self
                .graph
                .project_job(job.fence(), payload.space_id, &payload.sources)
                .await
                .map_err(retryable_domain_error)?;
            tracing::debug!(
                event = "link_graph.nodes_projected",
                space_id = %payload.space_id,
                requested = payload.sources.len(),
                projected = result.projected,
                failed = result.failed,
                removed = result.removed,
                skipped = result.skipped,
                stale = result.stale,
            );
            Ok(JobDisposition::Complete)
        })
    }
}

fn retryable_domain_error(error: notegate_core::Error) -> JobFailure {
    JobFailure::retryable("link_graph_projection_failed", error.to_string())
}
