use std::time::Duration;

use notegate_db::{PgPool, PurgeRepo};
use notegate_reconciliation::{
    Reconciler, ReconciliationContext, ReconciliationDirective, ReconciliationError,
    ReconciliationFailure, ReconciliationFuture, ReconciliationSchedule,
};

const PURGE_INTERVAL: Duration = Duration::from_secs(60 * 60);
const PURGE_TIMEOUT: Duration = Duration::from_secs(60 * 60);

pub(super) struct PurgeReconciler {
    repo: PurgeRepo,
}

impl PurgeReconciler {
    pub(super) fn new(pool: PgPool) -> Self {
        Self {
            repo: PurgeRepo::new(pool),
        }
    }

    pub(super) fn schedule() -> Result<ReconciliationSchedule, ReconciliationError> {
        ReconciliationSchedule::new(PURGE_INTERVAL, PURGE_TIMEOUT)
    }
}

impl Reconciler for PurgeReconciler {
    const KIND: &'static str = "system.purge";

    fn reconcile<'a>(&'a self, _context: &'a ReconciliationContext) -> ReconciliationFuture<'a> {
        Box::pin(async move {
            let run = self
                .repo
                .run_once()
                .await
                .map_err(|error| Box::new(error) as ReconciliationFailure)?;
            tracing::info!(
                event = "purge.completed",
                spaces_deleted = run.spaces_deleted,
                nodes_deleted = run.nodes_deleted,
                accounts_anonymized = run.accounts_anonymized,
                api_keys_deleted = run.api_keys_deleted,
                browser_sessions_deleted = run.browser_sessions_deleted,
                object_storage_history_deleted = run.object_storage_history_deleted,
                audit_events_deleted = run.audit_events_deleted,
                file_change_events_deleted = run.file_change_events_deleted,
                mcp_invocations_deleted = run.mcp_invocations_deleted,
                link_graph_targets_deleted = run.link_graph_targets_deleted,
                object_deletions_queued = run.object_deletions_queued,
            );
            Ok(ReconciliationDirective::Complete)
        })
    }
}
