//! Current Markdown link projection and Space indexing state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum LinkReferenceKind {
    Link,
    Image,
}

impl LinkReferenceKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "link" => Some(Self::Link),
            "image" => Some(Self::Image),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Image => "image",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkReferenceStatus {
    Resolved,
    Deleted,
    Missing,
    Invalid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkIndexStatus {
    Uninitialized,
    Queued,
    Running,
    Rebuilding,
    Ready,
    Failed,
}

impl LinkIndexStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "uninitialized" => Some(Self::Uninitialized),
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "rebuilding" => Some(Self::Rebuilding),
            "ready" => Some(Self::Ready),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceLinkIndexState {
    pub space_id: Uuid,
    pub desired_generation: i64,
    pub applied_generation: i64,
    pub status: LinkIndexStatus,
    pub last_indexed_at: Option<DateTime<Utc>>,
}

impl SpaceLinkIndexState {
    pub fn freshness(&self) -> LinkIndexFreshness {
        if self.status == LinkIndexStatus::Uninitialized {
            LinkIndexFreshness::Uninitialized
        } else if self.status == LinkIndexStatus::Rebuilding {
            LinkIndexFreshness::Rebuilding
        } else if self.status == LinkIndexStatus::Failed {
            LinkIndexFreshness::Failed
        } else if self.status == LinkIndexStatus::Ready
            && self.applied_generation == self.desired_generation
        {
            LinkIndexFreshness::Current
        } else {
            LinkIndexFreshness::Updating
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkIndexFreshness {
    Uninitialized,
    Current,
    Updating,
    Rebuilding,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkReference {
    pub id: i64,
    pub kind: LinkReferenceKind,
    pub status: LinkReferenceStatus,
    pub raw_href: String,
    pub normalized_target_path: Option<String>,
    pub occurrence_count: i32,
    pub source_node_id: Uuid,
    pub source_name: String,
    pub source_path: Option<String>,
    pub target_node_id: Option<Uuid>,
    pub target_name: Option<String>,
    pub target_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeLinkSummary {
    pub index: SpaceLinkIndexState,
    pub outgoing_count: i64,
    pub incoming_count: i64,
    pub broken_count: i64,
    pub outgoing: Vec<LinkReference>,
    pub incoming: Vec<LinkReference>,
    pub outgoing_truncated: bool,
    pub incoming_truncated: bool,
}
