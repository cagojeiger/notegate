//! Owner-only trash metadata. Content remains in its original storage.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashItem {
    pub id: Uuid,
    pub space_id: Uuid,
    pub space_name: String,
    pub kind: String,
    pub name: String,
    pub path: String,
    pub deleted_at: DateTime<Utc>,
    pub purge_after: DateTime<Utc>,
    pub deletion_operation_id: Option<Uuid>,
    pub recoverable: bool,
    pub deletion_pending: bool,
}

/// The deletion instance selected by the user, including legacy rows without an operation ID.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct TrashEntryVersion {
    pub deleted_at: DateTime<Utc>,
    pub deletion_operation_id: Option<Uuid>,
}

impl From<&TrashItem> for TrashEntryVersion {
    fn from(item: &TrashItem) -> Self {
        Self {
            deleted_at: item.deleted_at,
            deletion_operation_id: item.deletion_operation_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashCursor {
    pub owner_user_id: Uuid,
    pub deleted_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug, Clone)]
pub struct TrashPage {
    pub items: Vec<TrashItem>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}
