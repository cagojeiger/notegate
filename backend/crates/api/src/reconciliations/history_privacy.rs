//! Bounded encryption of retained rows written before history encryption existed.
use notegate_core::security::PiiCrypto;
use notegate_db::{ChangeHistoryRepo, CommandInvocationRepo, PgPool};
use notegate_reconciliation::{
    Reconciler, ReconciliationContext, ReconciliationDirective, ReconciliationError,
    ReconciliationFailure, ReconciliationFuture, ReconciliationSchedule,
};
use std::time::Duration;

pub(super) struct HistoryPrivacyReconciler {
    pool: PgPool,
    crypto: PiiCrypto,
    changes: ChangeHistoryRepo,
    invocations: CommandInvocationRepo,
}
impl HistoryPrivacyReconciler {
    pub(super) fn new(pool: PgPool, crypto: PiiCrypto) -> Self {
        Self {
            changes: ChangeHistoryRepo::new(pool.clone(), crypto.clone()),
            invocations: CommandInvocationRepo::with_crypto(pool.clone(), crypto.clone()),
            pool,
            crypto,
        }
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
                .changes
                .encrypt_legacy_metadata()
                .await
                .map_err(|error| Box::new(error) as ReconciliationFailure)?;
            let invocations = self
                .invocations
                .encrypt_legacy_payloads()
                .await
                .map_err(|error| Box::new(error) as ReconciliationFailure)?;
            let revisions =
                notegate_db::files::revisions::encrypt_legacy_purposes(&self.pool, &self.crypto)
                    .await
                    .map_err(|error| Box::new(error) as ReconciliationFailure)?;
            if count + invocations + revisions > 0 {
                tracing::info!(event = "history.encrypted", count, invocations, revisions);
            }
            Ok(if count == 100 || invocations == 100 || revisions >= 100 {
                ReconciliationDirective::ContinueAfter(Duration::from_secs(1))
            } else {
                ReconciliationDirective::Complete
            })
        })
    }
}
