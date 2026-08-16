use std::collections::BTreeMap;
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
    pub(crate) history_visibility: JobHistoryVisibility,
    pub(crate) history_owner_account_id: Option<Uuid>,
    pub(crate) history_context: Option<JobHistoryContext>,
    _job: PhantomData<fn() -> J>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobHistoryVisibility {
    Hidden,
    Visible,
}

impl JobHistoryVisibility {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Visible => "visible",
        }
    }
}

/// Optional display context attached to an account-scoped job history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobHistoryContext {
    pub kind: String,
    pub id: Option<Uuid>,
    pub label: Option<String>,
}

impl JobHistoryContext {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            id: None,
            label: None,
        }
    }

    pub fn id(mut self, id: Uuid) -> Self {
        self.id = Some(id);
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl<J: JobSpec> NewJob<J> {
    pub fn new(payload: J::Payload) -> Self {
        Self {
            payload,
            available_at: None,
            max_attempts: 8,
            history_visibility: JobHistoryVisibility::Hidden,
            history_owner_account_id: None,
            history_context: None,
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

    /// Record this job in the owning account's history.
    pub fn record_in_history(
        mut self,
        owner_account_id: Uuid,
        context: Option<JobHistoryContext>,
    ) -> Self {
        self.history_visibility = JobHistoryVisibility::Visible;
        self.history_owner_account_id = Some(owner_account_id);
        self.history_context = context;
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
pub struct RecoveryKindSummary {
    pub retried: u64,
    pub dead: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoverySummary {
    pub retried: u64,
    pub dead: u64,
    pub by_kind: BTreeMap<String, RecoveryKindSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobStateCount {
    pub kind: String,
    pub state: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobOldestReadyAt {
    pub kind: String,
    pub available_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobQueueSnapshot {
    pub states: Vec<JobStateCount>,
    pub oldest_ready: Vec<JobOldestReadyAt>,
}
