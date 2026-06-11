//! Search command and result data shared by service, db, and api.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::NodeKind;
use crate::files::NodeView;

#[derive(Debug, Clone)]
pub struct FindRequest {
    pub q: String,
    pub path: Option<String>,
    pub kind: Option<NodeKind>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindCursor {
    pub name: String,
    pub id: Uuid,
}

#[derive(Debug, Clone)]
pub struct GrepRequest {
    pub q: String,
    pub path: Option<String>,
    pub context: Option<i64>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrepCursor {
    pub updated_at: DateTime<Utc>,
    pub node_id: Uuid,
    pub match_offset: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepMatch {
    pub node_id: Uuid,
    pub path: String,
    pub line_no: i64,
    pub line: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FindPage {
    pub items: Vec<NodeView>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GrepPage {
    pub items: Vec<GrepMatch>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GrepCandidate {
    pub node_id: Uuid,
    pub path: String,
    pub content: String,
    pub updated_at: DateTime<Utc>,
}
