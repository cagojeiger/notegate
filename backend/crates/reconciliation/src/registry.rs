use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::{ErasedReconciler, Reconciler, ReconciliationError, ReconciliationSchedule};

const MAX_KIND_BYTES: usize = 128;
const LOCK_NAMESPACE: &[u8] = b"notegate.reconciliation.v1:";
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone)]
pub(crate) struct RegisteredReconciler {
    pub(crate) kind: &'static str,
    pub(crate) lock_key: i64,
    pub(crate) schedule: ReconciliationSchedule,
    pub(crate) reconciler: Arc<dyn ErasedReconciler>,
}

#[derive(Default)]
pub struct ReconciliationRegistry {
    entries: HashMap<&'static str, RegisteredReconciler>,
    lock_keys: HashSet<i64>,
}

impl ReconciliationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<R: Reconciler>(
        mut self,
        reconciler: R,
        schedule: ReconciliationSchedule,
    ) -> Result<Self, ReconciliationError> {
        validate_kind(R::KIND)?;
        let lock_key = advisory_lock_key(R::KIND);
        if self.entries.contains_key(R::KIND) {
            return Err(ReconciliationError::InvalidConfiguration(format!(
                "duplicate reconciliation kind: {}",
                R::KIND
            )));
        }
        if !self.lock_keys.insert(lock_key) {
            return Err(ReconciliationError::InvalidConfiguration(format!(
                "advisory lock key collision for reconciliation kind: {}",
                R::KIND
            )));
        }
        self.entries.insert(
            R::KIND,
            RegisteredReconciler {
                kind: R::KIND,
                lock_key,
                schedule,
                reconciler: Arc::new(reconciler),
            },
        );
        Ok(self)
    }

    pub(crate) fn into_entries(self) -> Vec<RegisteredReconciler> {
        let mut entries = self.entries.into_values().collect::<Vec<_>>();
        entries.sort_unstable_by_key(|entry| entry.kind);
        entries
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn validate_kind(kind: &str) -> Result<(), ReconciliationError> {
    if kind.is_empty() || kind.len() > MAX_KIND_BYTES {
        return Err(ReconciliationError::InvalidConfiguration(format!(
            "reconciliation kind must contain between 1 and {MAX_KIND_BYTES} bytes"
        )));
    }
    Ok(())
}

fn advisory_lock_key(kind: &str) -> i64 {
    let hash = LOCK_NAMESPACE
        .iter()
        .chain(kind.as_bytes())
        .fold(FNV_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
        });
    i64::from_be_bytes(hash.to_be_bytes())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use crate::{ReconciliationContext, ReconciliationFuture};

    use super::*;

    struct TestReconciler;

    impl Reconciler for TestReconciler {
        const KIND: &'static str = "test.reconcile";

        fn reconcile<'a>(
            &'a self,
            _context: &'a ReconciliationContext,
        ) -> ReconciliationFuture<'a> {
            Box::pin(async { Ok(()) })
        }
    }

    fn schedule() -> ReconciliationSchedule {
        ReconciliationSchedule::new(Duration::from_secs(60), Duration::from_secs(10)).unwrap()
    }

    #[test]
    fn rejects_duplicate_kinds() {
        let result = ReconciliationRegistry::new()
            .register(TestReconciler, schedule())
            .unwrap()
            .register(TestReconciler, schedule());

        assert!(matches!(
            result,
            Err(ReconciliationError::InvalidConfiguration(message))
                if message.contains("duplicate reconciliation kind")
        ));
    }

    #[test]
    fn lock_keys_are_stable_and_kind_scoped() {
        assert_eq!(
            advisory_lock_key("system.purge"),
            advisory_lock_key("system.purge")
        );
        assert_ne!(
            advisory_lock_key("system.purge"),
            advisory_lock_key("background_jobs.lease_recovery")
        );
    }
}
