//! Search command and result data shared by service, db, and api.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::files::{ChildrenCursor, NodeView};
use crate::{Node, NodeKind, TextObject};

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
    pub line_mode: GrepLineMode,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TreeRequest {
    pub path: Option<String>,
    pub depth: Option<i64>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrepLineMode {
    None,
    First,
    All,
}

impl GrepLineMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::First => "first",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCursor {
    pub version: u8,
    pub command: String,
    pub fingerprint: String,
    pub scope_node_id: Uuid,
    pub after_sort_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeCursor {
    pub version: u8,
    pub command: String,
    pub fingerprint: String,
    pub stack: Vec<TreeFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeFrame {
    pub folder_node_id: Uuid,
    pub depth: i64,
    pub after: Option<ChildrenCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchNodeCandidate {
    pub node: Node,
    pub path: String,
    pub sort_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTextCandidate {
    pub node: Node,
    pub path: String,
    pub sort_path: String,
    pub text: TextObject,
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
    pub items: Vec<GrepHit>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GrepHit {
    pub node: NodeView,
    pub match_lines: Vec<i32>,
}

#[derive(Debug, Clone)]
pub struct TreePage {
    pub items: Vec<NodeView>,
    pub depth: i64,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}
