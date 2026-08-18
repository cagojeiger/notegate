mod background_jobs;
mod link_graph;
mod object_storage;
mod purge;

use notegate_db::{LinkGraphWorkRepo, PgPool};
use notegate_jobs::JobQueue;
use notegate_reconciliation::{ReconciliationError, ReconciliationRegistry, ReconciliationRuntime};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::object_storage::ObjectStorage;

use background_jobs::{JobHistoryRetentionReconciler, LeaseRecoveryReconciler};
use link_graph::LinkGraphChangeCollector;
use object_storage::ObjectStorageCleanupReconciler;
use purge::PurgeReconciler;

#[cfg(test)]
pub(crate) use object_storage::run_once as run_object_storage_cleanup_once;

pub(crate) fn spawn(
    pool: &PgPool,
    object_storage: ObjectStorage,
    registered_job_kinds: &[String],
    shutdown: CancellationToken,
) -> Result<JoinHandle<()>, ReconciliationError> {
    let queue = JobQueue::new(pool.clone());
    let registry = ReconciliationRegistry::new()
        .register(
            PurgeReconciler::new(pool.clone()),
            PurgeReconciler::schedule()?,
        )?
        .register(
            LeaseRecoveryReconciler::new(queue.clone(), registered_job_kinds),
            LeaseRecoveryReconciler::schedule()?,
        )?
        .register(
            JobHistoryRetentionReconciler::new(queue),
            JobHistoryRetentionReconciler::schedule()?,
        )?
        .register(
            ObjectStorageCleanupReconciler::new(pool.clone(), object_storage),
            ObjectStorageCleanupReconciler::schedule()?,
        )?
        .register(
            LinkGraphChangeCollector::new(LinkGraphWorkRepo::new(pool.clone())),
            LinkGraphChangeCollector::schedule()?,
        )?;
    let runtime = ReconciliationRuntime::new(pool, registry)?;
    Ok(tokio::spawn(runtime.run(shutdown)))
}
