//! Bounded encryption of retained rows written before history encryption existed.
use notegate_db::ChangeHistoryRepo;
use notegate_reconciliation::{
    Reconciler, ReconciliationContext, ReconciliationDirective, ReconciliationError,
    ReconciliationFailure, ReconciliationFuture, ReconciliationSchedule,
};
use std::time::Duration;

pub(super) struct HistoryPrivacyReconciler {
    files: ChangeHistoryRepo,
}
impl HistoryPrivacyReconciler {
    pub(super) fn new(files: ChangeHistoryRepo) -> Self {
        Self { files }
    }
    pub(super) fn schedule() -> Result<ReconciliationSchedule, ReconciliationError> {
        ReconciliationSchedule::new(Duration::from_secs(60), Duration::from_secs(60))
    }
}
impl Reconciler for HistoryPrivacyReconciler {
    const KIND: &'static str = "history.encryption";
    fn reconcile<'a>(&'a self, _context: &'a ReconciliationContext) -> ReconciliationFuture<'a> {
        Box::pin(async move {
            let count = self
                .files
                .encrypt_legacy_metadata()
                .await
                .map_err(|error| Box::new(error) as ReconciliationFailure)?;
            if count > 0 {
                tracing::info!(event = "history.encrypted", count);
            }
            Ok(if count == 100 {
                ReconciliationDirective::ContinueAfter(Duration::from_secs(1))
            } else {
                ReconciliationDirective::Complete
            })
        })
    }
}
