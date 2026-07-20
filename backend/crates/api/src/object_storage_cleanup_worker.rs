//! Retryable cleanup for unattached and soft-deleted S3-compatible objects.

use std::time::Duration;

use notegate_db::{CleanupCandidate, ObjectStorageRepo, PgPool};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::object_storage::ObjectStorage;
use crate::periodic_worker;

const CLEANUP_INTERVAL: Duration = Duration::from_secs(60);
const DELETE_TIMEOUT: Duration = Duration::from_secs(10);
const STALE_UPLOAD_SECONDS: i64 = 2 * 60 * 60;
// A claimed row remains unavailable longer than one bounded S3 delete call.
const CLAIM_SECONDS: i64 = 30;
const CLEANUP_BATCH: i64 = 100;
const HISTORY_RETENTION_DAYS: i32 = 90;

pub fn spawn(pool: PgPool, storage: ObjectStorage, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(event = "object_storage_cleanup_worker.started");
        let drain_shutdown = shutdown.clone();
        periodic_worker::run(CLEANUP_INTERVAL, shutdown, || {
            let repo = ObjectStorageRepo::new(pool.clone());
            let storage = storage.clone();
            let shutdown = drain_shutdown.clone();
            async move { run_once(&repo, &storage, &shutdown).await }
        })
        .await;
        tracing::info!(event = "object_storage_cleanup_worker.stopped");
    })
}

pub(super) async fn run_once(
    repo: &ObjectStorageRepo,
    storage: &ObjectStorage,
    shutdown: &CancellationToken,
) {
    for _ in 0..CLEANUP_BATCH {
        if shutdown.is_cancelled() {
            break;
        }
        let candidate = match repo
            .claim_cleanup(STALE_UPLOAD_SECONDS, CLAIM_SECONDS)
            .await
        {
            Ok(Some(candidate)) => candidate,
            Ok(None) => break,
            Err(error) => {
                tracing::error!(event = "object_storage_cleanup.claim_failed", %error);
                return;
            }
        };
        if let Err(error) = process_candidate(repo, storage, &candidate).await {
            tracing::error!(
                event = "object_storage_cleanup.record_failed",
                object_key = %candidate.object_key,
                %error,
            );
        }
    }

    if shutdown.is_cancelled() {
        return;
    }

    match repo
        .purge_terminal_history(HISTORY_RETENTION_DAYS, CLEANUP_BATCH)
        .await
    {
        Ok(count) if count > 0 => {
            tracing::info!(event = "object_storage_cleanup.history_purged", count)
        }
        Ok(_) => {}
        Err(error) => {
            tracing::error!(event = "object_storage_cleanup.history_purge_failed", %error)
        }
    }
}

async fn process_candidate(
    repo: &ObjectStorageRepo,
    storage: &ObjectStorage,
    candidate: &CleanupCandidate,
) -> notegate_core::Result<()> {
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
        // A multipart complete may have succeeded before attachment failed.
        // Delete is idempotent and covers both that case and ordinary objects.
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
    use super::cleanup_retry_seconds;

    #[test]
    fn cleanup_retry_uses_bounded_exponential_backoff() {
        assert_eq!(cleanup_retry_seconds(-1), 30);
        assert_eq!(cleanup_retry_seconds(0), 30);
        assert_eq!(cleanup_retry_seconds(1), 60);
        assert_eq!(cleanup_retry_seconds(2), 120);
        assert_eq!(cleanup_retry_seconds(7), 3_600);
        assert_eq!(cleanup_retry_seconds(i32::MAX), 3_600);
    }
}
