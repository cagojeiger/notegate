//! Audit event history: read model for self-review event queries.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// One audit event row, scoped to the caller's own `owner_user_id` at read time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditEvent {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub actor_account_id: Option<Uuid>,
    pub source: String,
    pub op_type: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub metadata: Value,
}

/// Input to list the caller's own audit event history.
#[derive(Debug, Clone, Default)]
pub struct ListAuditEvents {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// Keyset cursor for audit event list order `(created_at DESC, id DESC)`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditEventCursor {
    pub created_at: DateTime<Utc>,
    pub id: i64,
}

#[derive(Debug, Clone)]
pub struct AuditEventPage {
    pub items: Vec<AuditEvent>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}
