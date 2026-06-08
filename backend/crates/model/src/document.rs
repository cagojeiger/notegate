//! Document content and its derived metrics. One document is keyed to one node.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The stored content of a document node, with size metrics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Document {
    pub node_id: Uuid,
    pub workspace_id: Uuid,
    pub content_md: String,
    pub content_sha256: String,
    pub byte_len: i32,
    pub line_count: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
