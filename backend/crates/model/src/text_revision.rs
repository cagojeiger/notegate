//! Historical text metadata and bounded history pages. Bodies are fetched separately.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRevision {
    pub id: Uuid,
    pub node_id: Uuid,
    pub content_sha256: String,
    pub byte_len: i64,
    pub line_count: i32,
    pub written_at: DateTime<Utc>,
    pub author_id: Uuid,
    pub group_id: Uuid,
    pub source: String,
    pub superseded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRevisionCursor {
    pub superseded_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct TextRevisionPage {
    pub revisions: Vec<TextRevision>,
    pub next_cursor: Option<TextRevisionCursor>,
}

#[derive(Debug, Serialize)]
pub struct TextRevisionContent {
    pub revision: TextRevision,
    pub content: String,
}
