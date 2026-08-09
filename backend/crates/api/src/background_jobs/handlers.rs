use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use notegate_db::{SPACE_USAGE_JOB_KIND, SpaceUsageRepo, UsageReconcileResult};
use notegate_jobs::{ClaimedJob, JobDisposition, JobFailure, JobHandler};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct UsageHandler {
    usage: SpaceUsageRepo,
}

impl UsageHandler {
    pub(crate) fn new(usage: SpaceUsageRepo) -> Self {
        Self { usage }
    }
}

impl JobHandler for UsageHandler {
    fn kind(&self) -> &'static str {
        SPACE_USAGE_JOB_KIND
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(60)
    }

    fn handle<'a>(
        &'a self,
        job: &'a ClaimedJob,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
        Box::pin(async move {
            let payload = decode_payload::<SpacePayload>(job)?;
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

#[derive(Debug, Deserialize)]
struct SpacePayload {
    space_id: Uuid,
}

fn decode_payload<T>(job: &ClaimedJob) -> Result<T, JobFailure>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(job.payload.clone()).map_err(|error| {
        JobFailure::permanent(
            "invalid_job_payload",
            format!("invalid {} payload: {error}", job.kind),
        )
    })
}

fn retryable_domain_error(error: notegate_core::Error) -> JobFailure {
    JobFailure::retryable("domain_error", error.to_string())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;

    #[test]
    fn payload_decoder_rejects_missing_identifiers_permanently() {
        let job = ClaimedJob {
            job_id: Uuid::new_v4(),
            kind: SPACE_USAGE_JOB_KIND.to_owned(),
            payload: json!({}),
            attempt: 1,
            failure_count: 0,
            max_attempts: 8,
            claim_token: Uuid::new_v4(),
            created_at: Utc::now(),
        };

        let failure = decode_payload::<SpacePayload>(&job).expect_err("payload must fail");
        assert_eq!(failure.class, notegate_jobs::JobFailureClass::Permanent);
        assert_eq!(failure.code, "invalid_job_payload");
    }
}
