//! File change event history: read model for space-scoped file-tree changes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::EventCursor;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileChangeEvent {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub space_id: Uuid,
    pub node_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub op_type: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, Default)]
pub struct ListFileChangeEvents {
    pub node_id: Option<Uuid>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

pub type FileChangeEventCursor = EventCursor;

#[derive(Debug, Clone)]
pub struct FileChangeEventPage {
    pub items: Vec<FileChangeEvent>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}
