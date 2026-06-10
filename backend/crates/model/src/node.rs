//! Tree nodes. The canonical location is `parent_id + name`; the display path
//! is derived from the parent chain and never stored on the node.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Whether a node is a folder or a document.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Folder,
    Document,
}

impl NodeKind {
    /// Parse the storage representation, returning `None` for unknown values.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "folder" => Some(Self::Folder),
            "document" => Some(Self::Document),
            _ => None,
        }
    }

    /// The storage representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Document => "document",
        }
    }
}

/// A tree node. `path` is intentionally absent — it is derived in the DTO layer
/// from the parent chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: NodeKind,
    pub sort_order: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub deleted_by: Option<Uuid>,
    pub purge_after: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
