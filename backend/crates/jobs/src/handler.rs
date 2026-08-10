use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::queue::validate_job_kind;
use crate::{ClaimedJob, JobDisposition, JobFailure, JobQueueError, JobQueueResult, JobSpec};

pub trait JobHandler<J: JobSpec>: Send + Sync + 'static {
    fn timeout(&self) -> Duration;
    fn handle<'a>(
        &'a self,
        job: &'a ClaimedJob,
        payload: J::Payload,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>>;
}

pub(crate) trait ErasedJobHandler: Send + Sync {
    fn timeout(&self) -> Duration;
    fn handle<'a>(
        &'a self,
        job: &'a ClaimedJob,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>>;
}

pub(crate) type HandlerMap = HashMap<&'static str, Arc<dyn ErasedJobHandler>>;

struct TypedJobHandler<J, H> {
    handler: H,
    _job: PhantomData<fn() -> J>,
}

impl<J, H> ErasedJobHandler for TypedJobHandler<J, H>
where
    J: JobSpec,
    H: JobHandler<J>,
{
    fn timeout(&self) -> Duration {
        self.handler.timeout()
    }

    fn handle<'a>(
        &'a self,
        job: &'a ClaimedJob,
    ) -> Pin<Box<dyn Future<Output = Result<JobDisposition, JobFailure>> + Send + 'a>> {
        let payload: J::Payload = match serde_json::from_value(job.payload.clone()) {
            Ok(payload) => payload,
            Err(error) => {
                let failure = JobFailure::permanent(
                    "invalid_job_payload",
                    format!("invalid {} payload: {error}", job.kind),
                );
                return Box::pin(async move { Err(failure) });
            }
        };
        self.handler.handle(job, payload)
    }
}

#[derive(Default)]
pub struct JobRegistry {
    handlers: HandlerMap,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<J: JobSpec>(mut self, handler: impl JobHandler<J>) -> JobQueueResult<Self> {
        validate_job_kind(J::KIND)?;
        if handler.timeout().is_zero() {
            return Err(JobQueueError::InvalidConfiguration(format!(
                "background job handler {} must have a positive timeout",
                J::KIND
            )));
        }
        let handler = Arc::new(TypedJobHandler::<J, _> {
            handler,
            _job: PhantomData,
        });
        if self.handlers.insert(J::KIND, handler).is_some() {
            return Err(JobQueueError::InvalidConfiguration(format!(
                "duplicate background job handler kind: {}",
                J::KIND
            )));
        }
        Ok(self)
    }

    pub fn job_kinds(&self) -> Vec<String> {
        let mut kinds = self
            .handlers
            .keys()
            .map(|kind| (*kind).to_owned())
            .collect::<Vec<_>>();
        kinds.sort_unstable();
        kinds
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    pub(crate) fn into_handlers(self) -> HandlerMap {
        self.handlers
    }
}
