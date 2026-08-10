use std::marker::PhantomData;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use uuid::Uuid;

pub trait JobSpec: Send + Sync + 'static {
    const KIND: &'static str;
    type Payload: Serialize + DeserializeOwned + Send + Sync + 'static;
}

pub struct NewJob<J: JobSpec> {
    pub payload: J::Payload,
    pub available_at: Option<DateTime<Utc>>,
    pub max_attempts: i32,
    _job: PhantomData<fn() -> J>,
}

impl<J: JobSpec> NewJob<J> {
    pub fn new(payload: J::Payload) -> Self {
        Self {
            payload,
            available_at: None,
            max_attempts: 8,
            _job: PhantomData,
        }
    }

    pub fn available_at(mut self, available_at: DateTime<Utc>) -> Self {
        self.available_at = Some(available_at);
        self
    }

    pub fn max_attempts(mut self, max_attempts: i32) -> Self {
        self.max_attempts = max_attempts;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnqueuedJob {
    pub job_id: Uuid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedJob {
    pub job_id: Uuid,
    pub kind: String,
    pub payload: Value,
    pub attempt: i32,
    pub failure_count: i32,
    pub max_attempts: i32,
    pub claim_token: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobDisposition {
    Complete,
    Defer {
        reason: &'static str,
        retry_after: Duration,
    },
}

impl ClaimedJob {
    pub fn fence(&self) -> ClaimFence {
        ClaimFence {
            job_id: self.job_id,
            claim_token: self.claim_token,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimFence {
    pub job_id: Uuid,
    pub claim_token: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobFailureClass {
    Retryable,
    Permanent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobFailure {
    pub class: JobFailureClass,
    pub code: String,
    pub message: String,
    pub retry_after: Option<Duration>,
}

impl JobFailure {
    pub fn retryable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            class: JobFailureClass::Retryable,
            code: code.into(),
            message: message.into(),
            retry_after: None,
        }
    }

    pub fn retryable_after(
        code: impl Into<String>,
        message: impl Into<String>,
        retry_after: Duration,
    ) -> Self {
        Self {
            class: JobFailureClass::Retryable,
            code: code.into(),
            message: message.into(),
            retry_after: Some(retry_after),
        }
    }

    pub fn permanent(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            class: JobFailureClass::Permanent,
            code: code.into(),
            message: message.into(),
            retry_after: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    RetryableError,
    PermanentError,
    TimedOut,
    Panicked,
    Cancelled,
}

impl AttemptOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RetryableError => "retryable_error",
            Self::PermanentError => "permanent_error",
            Self::TimedOut => "timed_out",
            Self::Panicked => "panicked",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoverySummary {
    pub retried: u64,
    pub dead: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobStateCount {
    pub kind: String,
    pub state: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobQueueSnapshot {
    pub states: Vec<JobStateCount>,
    pub oldest_ready_at: Option<DateTime<Utc>>,
}
