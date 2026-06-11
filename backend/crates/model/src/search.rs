//! Search command and result data shared by service, db, and api.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::NodeKind;
use crate::files::{ChildrenCursor, NodeView};

#[derive(Debug, Clone)]
pub struct FindRequest {
    pub q: String,
    pub path: Option<String>,
    pub kind: Option<NodeKind>,
    pub match_mode: FindMatchMode,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindMatchMode {
    Contains,
    Regex,
    Glob,
}

impl FindMatchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Regex => "regex",
            Self::Glob => "glob",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GrepRequest {
    pub q: String,
    pub path: Option<String>,
    pub match_mode: GrepMatchMode,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrepMatchMode {
    Literal,
    Regex,
}

impl GrepMatchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Regex => "regex",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCursor {
    pub version: u8,
    pub command: String,
    pub fingerprint: String,
    pub stack: Vec<DfsFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DfsFrame {
    pub folder_node_id: Uuid,
    pub after: Option<ChildrenCursor>,
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
    pub items: Vec<NodeView>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}
