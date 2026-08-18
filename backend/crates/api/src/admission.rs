//! Process-local admission control for CPU-sensitive operations.

use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use notegate_core::limits;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchCapacity {
    Find,
    Grep,
}

#[derive(Clone)]
pub(crate) struct SearchAdmission {
    find: Arc<Semaphore>,
    grep_requests: Arc<Semaphore>,
    grep_executions: Arc<Semaphore>,
}

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

impl Default for SearchAdmission {
    fn default() -> Self {
        Self::new(
            limits::FIND_MAX_IN_FLIGHT,
            limits::GREP_MAX_IN_FLIGHT,
            limits::GREP_MAX_EXECUTING,
        )
    }
}

impl SearchAdmission {
    fn new(find: usize, grep_requests: usize, grep_executions: usize) -> Self {
        Self {
            find: Arc::new(Semaphore::new(find)),
            grep_requests: Arc::new(Semaphore::new(grep_requests)),
            grep_executions: Arc::new(Semaphore::new(grep_executions)),
        }
    }

    pub(crate) fn enter_find(&self) -> Result<OwnedSemaphorePermit, SearchCapacity> {
        self.find
            .clone()
            .try_acquire_owned()
            .map_err(|_| SearchCapacity::Find)
    }

    pub(crate) async fn enter_grep(&self) -> Result<GrepPermit, SearchCapacity> {
        let request = self
            .grep_requests
            .clone()
            .try_acquire_owned()
            .map_err(|_| SearchCapacity::Grep)?;
        let execution = self
            .grep_executions
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| SearchCapacity::Grep)?;
        Ok(GrepPermit {
            _request: request,
            _execution: execution,
        })
    }
}

pub(crate) struct GrepPermit {
    _request: OwnedSemaphorePermit,
    _execution: OwnedSemaphorePermit,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::time::Duration;

    use super::*;

    #[test]
    fn find_fails_fast_at_capacity_and_recovers_after_release() {
        let admission = SearchAdmission::new(2, 2, 1);
        let first = admission.enter_find().expect("first find admitted");
        let second = admission.enter_find().expect("second find admitted");

        assert!(matches!(admission.enter_find(), Err(SearchCapacity::Find)));

        drop(first);
        assert!(admission.enter_find().is_ok());
        drop(second);
    }

    #[tokio::test]
    async fn grep_bounds_waiting_requests_and_execution() {
        let admission = SearchAdmission::new(2, 2, 1);
        let first = admission.enter_grep().await.expect("first grep admitted");
        let waiting_admission = admission.clone();
        let waiting = tokio::spawn(async move { waiting_admission.enter_grep().await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while admission.grep_requests.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("waiting grep reserves the remaining request slot");

        assert!(!waiting.is_finished(), "second grep waits for the worker");
        assert!(matches!(
            admission.enter_grep().await,
            Err(SearchCapacity::Grep)
        ));

        drop(first);
        let second = tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("waiting grep starts after capacity is released")
            .expect("waiting task joins")
            .expect("waiting grep is admitted");
        drop(second);
    }

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
