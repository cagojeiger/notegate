//! Typed runtime for idempotent, periodic reconciliation work.

mod registry;
mod runtime;

use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use thiserror::Error;
use uuid::Uuid;

pub use registry::ReconciliationRegistry;
pub use runtime::ReconciliationRuntime;

pub type ReconciliationFailure = Box<dyn Error + Send + Sync>;
pub type ReconciliationResult = Result<ReconciliationDirective, ReconciliationFailure>;
pub type ReconciliationFuture<'a> = Pin<Box<dyn Future<Output = ReconciliationResult> + Send + 'a>>;

/// Scheduling feedback from one successful reconciliation run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationDirective {
    /// Resume the reconciler's registered fixed schedule.
    Complete,
    /// Run this kind again after a shorter delay because bounded work remains.
    ContinueAfter(Duration),
}

/// One idempotent convergence operation registered with the runtime.
///
/// Implementations must read current state and remain safe when invoked more
/// than once. The runtime prevents concurrent execution of the same kind, but
/// it does not promise exactly-once scheduling across process restarts.
pub trait Reconciler: Send + Sync + 'static {
    const KIND: &'static str;

    fn reconcile<'a>(&'a self, context: &'a ReconciliationContext) -> ReconciliationFuture<'a>;
}

/// Per-run context shared with application reconciliation code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconciliationContext {
    run_id: Uuid,
}

impl ReconciliationContext {
    pub fn run_id(&self) -> Uuid {
        self.run_id
    }
}

/// Fixed execution policy for one reconciliation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconciliationSchedule {
    interval: Duration,
    timeout: Duration,
}

impl ReconciliationSchedule {
    pub fn new(interval: Duration, timeout: Duration) -> Result<Self, ReconciliationError> {
        if interval.is_zero() {
            return Err(ReconciliationError::InvalidConfiguration(
                "reconciliation interval must be positive".to_owned(),
            ));
        }
        if timeout.is_zero() {
            return Err(ReconciliationError::InvalidConfiguration(
                "reconciliation timeout must be positive".to_owned(),
            ));
        }
        Ok(Self { interval, timeout })
    }

    pub fn interval(self) -> Duration {
        self.interval
    }

    pub fn timeout(self) -> Duration {
        self.timeout
    }
}

#[derive(Debug, Error)]
pub enum ReconciliationError {
    #[error("invalid reconciliation configuration: {0}")]
    InvalidConfiguration(String),
    #[error("reconciliation database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("advisory lock {0} was not held when released")]
    AdvisoryLockNotHeld(i64),
}

pub(crate) trait ErasedReconciler: Send + Sync {
    fn reconcile<'a>(&'a self, context: &'a ReconciliationContext) -> ReconciliationFuture<'a>;
}

impl<R: Reconciler> ErasedReconciler for R {
    fn reconcile<'a>(&'a self, context: &'a ReconciliationContext) -> ReconciliationFuture<'a> {
        Reconciler::reconcile(self, context)
    }
}
