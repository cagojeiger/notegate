use notegate_db::PgPool;
use notegate_reconciliation::{
    Reconciler, ReconciliationContext, ReconciliationDirective, ReconciliationError,
    ReconciliationFailure, ReconciliationFuture, ReconciliationSchedule,
};
use std::time::Duration;

pub(super) struct TextRevisionRetentionReconciler {
    pool: PgPool,
}
impl TextRevisionRetentionReconciler {
    pub(super) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
    pub(super) fn schedule() -> Result<ReconciliationSchedule, ReconciliationError> {
        ReconciliationSchedule::new(Duration::from_secs(600), Duration::from_secs(60))
    }
}
impl Reconciler for TextRevisionRetentionReconciler {
    const KIND: &'static str = "text_revisions.retention";
    fn reconcile<'a>(&'a self, _context: &'a ReconciliationContext) -> ReconciliationFuture<'a> {
        Box::pin(async move {
            let deleted = notegate_db::files::revisions::cleanup(&self.pool)
                .await
                .map_err(|e| Box::new(e) as ReconciliationFailure)?;
            tracing::info!(event = "text_revisions.cleaned", deleted);
            Ok(if deleted > 0 {
                ReconciliationDirective::ContinueAfter(Duration::from_secs(1))
            } else {
                ReconciliationDirective::Complete
            })
        })
    }
}
