use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use notegate_db::{
    SpaceUsagePayload, SpaceUsageReconcileJob, SpaceUsageRepo, UsageReconcileResult,
};
use notegate_jobs::{ClaimedJob, JobDisposition, JobFailure, JobHandler};

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

fn retryable_domain_error(error: notegate_core::Error) -> JobFailure {
    JobFailure::retryable("domain_error", error.to_string())
}
