//! Process-local admission control for CPU-sensitive operations.

use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use notegate_core::limits;

#[derive(Clone)]
pub(crate) struct DocxValidationAdmission {
    executions: Arc<Semaphore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocxValidationCapacity;

impl Default for DocxValidationAdmission {
    fn default() -> Self {
        Self::new(limits::DOCX_VALIDATION_MAX_EXECUTING)
    }
}

impl DocxValidationAdmission {
    pub(crate) fn new(executions: usize) -> Self {
        Self {
            executions: Arc::new(Semaphore::new(executions)),
        }
    }

    pub(crate) fn enter(&self) -> Result<OwnedSemaphorePermit, DocxValidationCapacity> {
        self.executions
            .clone()
            .try_acquire_owned()
            .map_err(|_| DocxValidationCapacity)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn docx_validation_fails_fast_at_capacity_and_recovers_after_release() {
        let admission = DocxValidationAdmission::new(2);
        let first = admission.enter().expect("first validation admitted");
        let second = admission.enter().expect("second validation admitted");

        assert!(admission.enter().is_err());

        drop(first);
        assert!(admission.enter().is_ok());
        drop(second);
    }
}
