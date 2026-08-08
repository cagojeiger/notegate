use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkReferenceKind {
    Link,
    Image,
}

impl LinkReferenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Image => "image",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "link" => Some(Self::Link),
            "image" => Some(Self::Image),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkSyncStatus {
    UpToDate,
    Pending,
    Syncing,
    Retrying,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkReferenceView {
    pub node_id: Option<Uuid>,
    pub path: String,
    pub kind: LinkReferenceKind,
    pub occurrence_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeLinkIndexView {
    pub status: LinkSyncStatus,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct ListLinkReferences {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingLinkCursor {
    pub space_id: Uuid,
    pub source_node_id: Uuid,
    pub target_path: String,
    pub kind: LinkReferenceKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingLinkCursor {
    pub space_id: Uuid,
    pub target_node_id: Uuid,
    pub source_node_id: Uuid,
    pub kind: LinkReferenceKind,
}

#[derive(Debug, Clone)]
pub struct LinkReferencePage {
    pub items: Vec<LinkReferenceView>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceLinkIndexView {
    pub status: LinkSyncStatus,
    pub pending_documents: i64,
    pub retrying_documents: i64,
    pub last_synced_at: Option<DateTime<Utc>>,
}
