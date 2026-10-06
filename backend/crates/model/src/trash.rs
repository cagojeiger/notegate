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
    pub recoverable: bool,
    pub deletion_pending: bool,
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
