//! Background job history read models.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackgroundJob {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub context_kind: Option<String>,
    pub context_id: Option<Uuid>,
    pub context_label: Option<String>,
    pub attempt_count: i32,
    pub failure_count: i32,
    pub max_attempts: i32,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackgroundJobAttempt {
    pub attempt_number: i32,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub outcome: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackgroundJobDetail {
    pub job: BackgroundJob,
    pub attempts: Vec<BackgroundJobAttempt>,
}

#[derive(Debug, Clone, Default)]
pub struct ListBackgroundJobs {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackgroundJobCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug, Clone)]
pub struct BackgroundJobPage {
    pub items: Vec<BackgroundJob>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}
