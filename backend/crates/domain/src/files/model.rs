use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Folder,
    Document,
}

impl NodeKind {
    pub fn from_storage(value: &str) -> Self {
        match value {
            "document" => Self::Document,
            _ => Self::Folder,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "folder" => Some(Self::Folder),
            "document" => Some(Self::Document),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Document => "document",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: NodeKind,
    pub path: String,
    pub sort_order: i32,
    pub has_children: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Children {
    pub parent: Node,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub node_id: Uuid,
    pub workspace_id: Uuid,
    pub content_md: String,
    pub content_sha256: String,
    pub byte_len: i32,
    pub line_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DocumentBundle {
    pub node: Node,
    pub document: Document,
}

#[derive(Debug, Clone)]
pub struct FindQuery {
    pub q: String,
    pub path: Option<String>,
    pub kind: Option<NodeKind>,
    pub limit: i64,
}

#[derive(Debug, Clone)]
pub struct GrepCandidateQuery {
    pub q: String,
    pub path: Option<String>,
    pub limit: i64,
}

#[derive(Debug, Clone)]
pub struct GrepCandidate {
    pub node_id: Uuid,
    pub path: String,
    pub content_md: String,
}

#[derive(Debug, Clone)]
pub struct GrepMatch {
    pub node_id: Uuid,
    pub path: String,
    pub line_no: i64,
    pub line: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}
