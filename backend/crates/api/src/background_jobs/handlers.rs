use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use notegate_db::{
    LinkExpansion, LinkImpactJob, LinkImpactPayload, LinkSourceJob, LinkSourcePayload,
    LinkSpaceJob, LinkSpacePayload, SpaceUsagePayload, SpaceUsageReconcileJob, SpaceUsageRepo,
    UsageReconcileResult,
};
use notegate_jobs::{ClaimedJob, JobDisposition, JobFailure, JobHandler};
use notegate_service::link_index::{LinkIndexService, LinkSourceWorkResult};

#[derive(Clone)]
pub(crate) struct UsageHandler {
    usage: SpaceUsageRepo,
}

impl UsageHandler {
    pub(crate) fn new(usage: SpaceUsageRepo) -> Self {
        Self { usage }
    }
}

impl JobHandler<SpaceUsageReconcileJob> for UsageHandler {
    fn timeout(&self) -> Duration {
        Duration::from_secs(60)
    }

    fn handle<'a>(
        &'a self,
        _job: &'a ClaimedJob,
        payload: SpaceUsagePayload,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
        Box::pin(async move {
            match self
                .usage
                .reconcile_space(payload.space_id)
                .await
                .map_err(retryable_domain_error)?
            {
                UsageReconcileResult::Busy => Ok(JobDisposition::Defer {
                    reason: "space_usage_busy",
                    retry_after: Duration::from_secs(5),
                }),
                UsageReconcileResult::Deleted => {
                    tracing::debug!(event = "space_usage.reconcile_deleted", %payload.space_id);
                    Ok(JobDisposition::Complete)
                }
                UsageReconcileResult::Reconciled { previous, actual } => {
                    tracing::info!(
                        event = "space_usage.reconciled",
                        space_id = %payload.space_id,
                        changed = previous != Some(actual),
                        counter_was_missing = previous.is_none(),
                        previous_nodes = previous.map(|counts| counts.live_node_count),
                        actual_nodes = actual.live_node_count,
                        previous_text_bytes = previous.map(|counts| counts.live_text_bytes),
                        actual_text_bytes = actual.live_text_bytes,
                        previous_file_bytes = previous.map(|counts| counts.live_file_bytes),
                        actual_file_bytes = actual.live_file_bytes,
                    );
                    Ok(JobDisposition::Complete)
                }
            }
        })
    }
}

#[derive(Clone)]
pub(crate) struct LinkSpaceHandler {
    links: LinkIndexService,
}

impl LinkSpaceHandler {
    pub(crate) fn new(links: LinkIndexService) -> Self {
        Self { links }
    }
}

impl JobHandler<LinkSpaceJob> for LinkSpaceHandler {
    fn timeout(&self) -> Duration {
        Duration::from_secs(5 * 60)
    }

    fn handle<'a>(
        &'a self,
        job: &'a ClaimedJob,
        payload: LinkSpacePayload,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
        Box::pin(async move {
            let fence = job.fence();
            match self
                .links
                .process_space(&fence, payload.space_id)
                .await
                .map_err(retryable_domain_error)?
            {
                LinkExpansion::Expanded => {
                    tracing::debug!(event = "link_index.space_expanded", %payload.space_id);
                    Ok(JobDisposition::Complete)
                }
                LinkExpansion::Deleted => {
                    tracing::debug!(event = "link_index.space_deleted", %payload.space_id);
                    Ok(JobDisposition::Complete)
                }
                LinkExpansion::ClaimLost => {
                    tracing::debug!(event = "link_index.claim_lost", %payload.space_id);
                    Ok(JobDisposition::Complete)
                }
            }
        })
    }
}

#[derive(Clone)]
pub(crate) struct LinkImpactHandler {
    links: LinkIndexService,
}

impl LinkImpactHandler {
    pub(crate) fn new(links: LinkIndexService) -> Self {
        Self { links }
    }
}

impl JobHandler<LinkImpactJob> for LinkImpactHandler {
    fn timeout(&self) -> Duration {
        Duration::from_secs(2 * 60)
    }

    fn handle<'a>(
        &'a self,
        job: &'a ClaimedJob,
        payload: LinkImpactPayload,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
        Box::pin(async move {
            let fence = job.fence();
            match self
                .links
                .process_impact(&fence, payload.space_id, payload.changed_node_id)
                .await
                .map_err(retryable_domain_error)?
            {
                LinkExpansion::Expanded => {
                    tracing::debug!(
                        event = "link_index.impact_expanded",
                        %payload.space_id,
                        %payload.changed_node_id,
                    );
                    Ok(JobDisposition::Complete)
                }
                LinkExpansion::Deleted => {
                    tracing::debug!(
                        event = "link_index.impact_deleted",
                        %payload.space_id,
                        %payload.changed_node_id,
                    );
                    Ok(JobDisposition::Complete)
                }
                LinkExpansion::ClaimLost => {
                    tracing::debug!(
                        event = "link_index.claim_lost",
                        %payload.space_id,
                        %payload.changed_node_id,
                    );
                    Ok(JobDisposition::Complete)
                }
            }
        })
    }
}

#[derive(Clone)]
pub(crate) struct LinkSourceHandler {
    links: LinkIndexService,
}

impl LinkSourceHandler {
    pub(crate) fn new(links: LinkIndexService) -> Self {
        Self { links }
    }
}

impl JobHandler<LinkSourceJob> for LinkSourceHandler {
    fn timeout(&self) -> Duration {
        Duration::from_secs(2 * 60)
    }

    fn handle<'a>(
        &'a self,
        job: &'a ClaimedJob,
        payload: LinkSourcePayload,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
        Box::pin(async move {
            let fence = job.fence();
            match self
                .links
                .process_source(&fence, payload.space_id, payload.source_node_id)
                .await
                .map_err(retryable_domain_error)?
            {
                LinkSourceWorkResult::Applied { reference_count } => {
                    tracing::debug!(
                        event = "link_index.source_indexed",
                        %payload.space_id,
                        %payload.source_node_id,
                        reference_count,
                    );
                    Ok(JobDisposition::Complete)
                }
                LinkSourceWorkResult::Deleted => {
                    tracing::debug!(
                        event = "link_index.source_deleted",
                        %payload.space_id,
                        %payload.source_node_id,
                    );
                    Ok(JobDisposition::Complete)
                }
                LinkSourceWorkResult::Stale => {
                    tracing::debug!(
                        event = "link_index.source_stale",
                        %payload.space_id,
                        %payload.source_node_id,
                    );
                    Ok(JobDisposition::Complete)
                }
                LinkSourceWorkResult::ClaimLost => {
                    tracing::debug!(
                        event = "link_index.claim_lost",
                        %payload.space_id,
                        %payload.source_node_id,
                    );
                    Ok(JobDisposition::Complete)
                }
            }
        })
    }
}

fn retryable_domain_error(error: notegate_core::Error) -> JobFailure {
    JobFailure::retryable("domain_error", error.to_string())
}
