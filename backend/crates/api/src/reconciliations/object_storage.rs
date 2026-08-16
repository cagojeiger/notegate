use std::time::Duration;

use notegate_core::Result as CoreResult;
use notegate_db::{CleanupCandidate, ObjectStorageRepo, PgPool};
use notegate_reconciliation::{
    Reconciler, ReconciliationContext, ReconciliationDirective, ReconciliationError,
    ReconciliationFailure, ReconciliationFuture, ReconciliationSchedule,
};

use crate::object_storage::ObjectStorage;

const CLEANUP_INTERVAL: Duration = Duration::from_secs(60);
const CLEANUP_RUN_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const CLEANUP_CONTINUATION_DELAY: Duration = Duration::from_secs(1);
const DELETE_TIMEOUT: Duration = Duration::from_secs(10);
const STALE_UPLOAD_SECONDS: i64 = 2 * 60 * 60;
const CLAIM_SECONDS: i64 = 30;
const CLEANUP_BATCH: usize = 100;

pub(super) struct ObjectStorageCleanupReconciler {
    repo: ObjectStorageRepo,
    storage: ObjectStorage,
}

impl ObjectStorageCleanupReconciler {
    pub(super) fn new(pool: PgPool, storage: ObjectStorage) -> Self {
        Self {
            repo: ObjectStorageRepo::new(pool),
            storage,
        }
    }

    pub(super) fn schedule() -> std::result::Result<ReconciliationSchedule, ReconciliationError> {
        ReconciliationSchedule::new(CLEANUP_INTERVAL, CLEANUP_RUN_TIMEOUT)
    }
}

impl Reconciler for ObjectStorageCleanupReconciler {
    const KIND: &'static str = "object_storage.cleanup";

    fn reconcile<'a>(&'a self, _context: &'a ReconciliationContext) -> ReconciliationFuture<'a> {
        Box::pin(async move {
            let processed = run_once(&self.repo, &self.storage)
                .await
                .map_err(|error| Box::new(error) as ReconciliationFailure)?;
            Ok(cleanup_directive(processed))
        })
    }
}

pub(crate) async fn run_once(
    repo: &ObjectStorageRepo,
    storage: &ObjectStorage,
) -> CoreResult<usize> {
    let mut first_error = None;
    let mut processed = 0;
    for _ in 0..CLEANUP_BATCH {
        let Some(candidate) = repo
            .claim_cleanup(STALE_UPLOAD_SECONDS, CLAIM_SECONDS)
            .await?
        else {
            break;
        };
        processed += 1;
        if let Err(error) = process_candidate(repo, storage, &candidate).await {
            tracing::error!(
                event = "object_storage_cleanup.record_failed",
                object_key = %candidate.object_key,
                %error,
            );
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(processed)
}

fn cleanup_directive(processed: usize) -> ReconciliationDirective {
    if processed == CLEANUP_BATCH {
        ReconciliationDirective::ContinueAfter(CLEANUP_CONTINUATION_DELAY)
    } else {
        ReconciliationDirective::Complete
    }
}

async fn process_candidate(
    repo: &ObjectStorageRepo,
    storage: &ObjectStorage,
    candidate: &CleanupCandidate,
) -> CoreResult<()> {
    let terminal_state = match candidate.state.as_str() {
        "uploading" => {
            if !repo.begin_expiry(candidate.id).await? {
                return Ok(());
            }
            "expired"
        }
        "expire_pending" => "expired",
        "delete_pending" => "deleted",
        _ => return Ok(()),
    };

    let cleanup = async {
        if candidate.upload_mode == "multipart"
            && let Some(upload_id) = candidate.multipart_upload_id.as_deref()
        {
            storage
                .abort_multipart_upload(&candidate.object_key, upload_id)
                .await?;
        }
        storage.delete(&candidate.object_key).await
    };
    let delete_error_code = match tokio::time::timeout(DELETE_TIMEOUT, cleanup).await {
        Ok(Ok(())) => None,
        Ok(Err(_error)) => Some("unavailable"),
        Err(_elapsed) => Some("timeout"),
    };

    match delete_error_code {
        None => {
            let recorded = if terminal_state == "expired" {
                repo.mark_expired(candidate.id).await?
            } else {
                repo.mark_deleted(candidate.id).await?
            };
            if !recorded {
                tracing::warn!(
                    event = "object_storage.cleanup_state_changed",
                    object_key = %candidate.object_key,
                    terminal_state,
                );
                return Ok(());
            }
            tracing::info!(
                event = "object_storage.cleanup_succeeded",
                object_key = %candidate.object_key,
                terminal_state,
            );
        }
        Some(error_code) => {
            let retry_seconds = cleanup_retry_seconds(candidate.retry_count);
            let _ = repo
                .mark_cleanup_failed(candidate.id, error_code, retry_seconds)
                .await?;
            tracing::warn!(
                event = "object_storage.cleanup_failed",
                object_key = %candidate.object_key,
                error_code,
                retry_seconds,
            );
        }
    }
    Ok(())
}

fn cleanup_retry_seconds(retry_count: i32) -> i64 {
    let exponent = u32::try_from(retry_count.clamp(0, 8)).unwrap_or(8);
    (30_i64.saturating_mul(2_i64.saturating_pow(exponent))).min(60 * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_retry_uses_bounded_exponential_backoff() {
        assert_eq!(cleanup_retry_seconds(-1), 30);
        assert_eq!(cleanup_retry_seconds(0), 30);
        assert_eq!(cleanup_retry_seconds(1), 60);
        assert_eq!(cleanup_retry_seconds(2), 120);
        assert_eq!(cleanup_retry_seconds(7), 3_600);
        assert_eq!(cleanup_retry_seconds(i32::MAX), 3_600);
    }

    #[test]
    fn a_full_cleanup_batch_requests_a_short_continuation() {
        assert_eq!(
            cleanup_directive(CLEANUP_BATCH - 1),
            ReconciliationDirective::Complete
        );
        assert_eq!(
            cleanup_directive(CLEANUP_BATCH),
            ReconciliationDirective::ContinueAfter(CLEANUP_CONTINUATION_DELAY)
        );
    }
}
