//! MCP invocation history read model for user self-review.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::EventCursor;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpInvocation {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub actor_account_id: Uuid,
    pub caller_kind: String,
    pub tool: String,
    pub op: Option<String>,
    pub purpose: Option<String>,
    pub space_name: Option<String>,
    pub input: Value,
    pub response: Option<Value>,
    pub outcome: String,
    pub error_code: Option<String>,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ListMcpInvocations {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

pub type McpInvocationCursor = EventCursor;

#[derive(Debug, Clone)]
pub struct McpInvocationPage {
    pub items: Vec<McpInvocation>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}
