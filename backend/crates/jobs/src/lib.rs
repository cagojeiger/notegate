//! PostgreSQL-backed at-least-once jobs and their worker runtime.

mod error;
mod handler;
mod model;
mod queue;
mod reconciler;
mod schedule;
mod worker;

pub use error::{JobQueueError, JobQueueResult};
pub use handler::{JobHandler, JobRegistry};
pub use model::{
    AttemptOutcome, ClaimFence, ClaimedJob, EnqueuedJob, JobDisposition, JobFailure,
    JobFailureClass, JobHistoryContext, JobQueueSnapshot, JobSpec, JobStateCount, NewJob,
    RecoverySummary,
};
pub use queue::{BACKGROUND_JOB_NOTIFY_CHANNEL, DeferTransition, FailureTransition, JobQueue};
pub use reconciler::{QueueReconciler, QueueReconcilerConfig};
pub use worker::{Worker, WorkerConfig};
