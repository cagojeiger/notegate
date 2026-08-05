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
    pub outgoing: Vec<LinkReferenceView>,
    pub incoming: Vec<LinkReferenceView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceLinkIndexView {
    pub status: LinkSyncStatus,
    pub pending_documents: i64,
    pub retrying_documents: i64,
    pub last_synced_at: Option<DateTime<Utc>>,
}
