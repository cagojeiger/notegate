use notegate_core::WriteLockScope;
use notegate_model::NodeKind;
use notegate_search::{
    FindMatchMode, FindPage, FindRequest, GrepLineMode, GrepMatchMode, GrepPage, GrepRequest,
    SearchCapacity, SearchError,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::path_node_summary::PathNodeSummary;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FindCommand {
    pub caller_account_id: Uuid,
    pub space_id: Uuid,
    pub q: String,
    pub path: Option<String>,
    pub kind: Option<NodeKind>,
    pub match_mode: FindMatchMode,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

impl FindCommand {
    pub(super) fn new(caller_account_id: Uuid, space_id: Uuid, request: FindRequest) -> Self {
        Self {
            caller_account_id,
            space_id,
            q: request.q,
            path: request.path,
            kind: request.kind,
            match_mode: request.match_mode,
            include: request.include,
            exclude: request.exclude,
            limit: request.limit,
            cursor: request.cursor,
        }
    }

    pub(super) fn into_request(self) -> FindRequest {
        FindRequest {
            q: self.q,
            path: self.path,
            kind: self.kind,
            match_mode: self.match_mode,
            include: self.include,
            exclude: self.exclude,
            limit: self.limit,
            cursor: self.cursor,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GrepCommand {
    pub caller_account_id: Uuid,
    pub space_id: Uuid,
    pub q: String,
    pub path: Option<String>,
    pub match_mode: GrepMatchMode,
    pub line_mode: GrepLineMode,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

impl GrepCommand {
    pub(super) fn new(caller_account_id: Uuid, space_id: Uuid, request: GrepRequest) -> Self {
        Self {
            caller_account_id,
            space_id,
            q: request.q,
            path: request.path,
            match_mode: request.match_mode,
            line_mode: request.line_mode,
            include: request.include,
            exclude: request.exclude,
            limit: request.limit,
            cursor: request.cursor,
        }
    }

    pub(super) fn into_request(self) -> GrepRequest {
        GrepRequest {
            q: self.q,
            path: self.path,
            match_mode: self.match_mode,
            line_mode: self.line_mode,
            include: self.include,
            exclude: self.exclude,
            limit: self.limit,
            cursor: self.cursor,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct FindOutput {
    pub items: Vec<PathNodeSummary>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

impl From<FindPage> for FindOutput {
    fn from(page: FindPage) -> Self {
        Self {
            items: page.items.into_iter().map(PathNodeSummary::from).collect(),
            limit: page.limit,
            has_more: page.has_more,
            next_cursor: page.next_cursor,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GrepOutput {
    pub items: Vec<GrepSummary>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

impl From<GrepPage> for GrepOutput {
    fn from(page: GrepPage) -> Self {
        Self {
            items: page
                .items
                .into_iter()
                .map(|hit| GrepSummary {
                    node: hit.node.into(),
                    match_lines: hit.match_lines,
                })
                .collect(),
            limit: page.limit,
            has_more: page.has_more,
            next_cursor: page.next_cursor,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GrepSummary {
    #[serde(flatten)]
    pub node: PathNodeSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_lines: Vec<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ErrorOutput {
    pub error: InternalSearchError,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum InternalSearchError {
    NotFound { message: String },
    InvalidInput { message: String },
    Forbidden { message: String },
    Conflict { message: String },
    WriteLocked { scope: WriteLockScopeWire },
    UsageRecalculationInProgress { retry_after_seconds: u64 },
    Busy { operation: SearchOperationWire },
    Internal,
}

impl InternalSearchError {
    pub(super) fn from_search(error: SearchError) -> Self {
        match error {
            SearchError::NotFound(message) => Self::NotFound { message },
            SearchError::InvalidInput(message) => Self::InvalidInput { message },
            SearchError::Forbidden(message) => Self::Forbidden { message },
            SearchError::Conflict(message) => Self::Conflict { message },
            SearchError::WriteLocked { scope } => Self::WriteLocked {
                scope: scope.into(),
            },
            SearchError::UsageRecalculationInProgress {
                retry_after_seconds,
            } => Self::UsageRecalculationInProgress {
                retry_after_seconds,
            },
            SearchError::Internal(_) => Self::Internal,
        }
    }

    pub(super) fn busy(capacity: SearchCapacity) -> Self {
        Self::Busy {
            operation: capacity.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SearchOperationWire {
    Find,
    Grep,
}

impl From<SearchCapacity> for SearchOperationWire {
    fn from(value: SearchCapacity) -> Self {
        match value {
            SearchCapacity::Find => Self::Find,
            SearchCapacity::Grep => Self::Grep,
        }
    }
}

impl From<SearchOperationWire> for SearchCapacity {
    fn from(value: SearchOperationWire) -> Self {
        match value {
            SearchOperationWire::Find => Self::Find,
            SearchOperationWire::Grep => Self::Grep,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum WriteLockScopeWire {
    TargetOrAncestor,
    Descendant,
}

impl From<WriteLockScope> for WriteLockScopeWire {
    fn from(value: WriteLockScope) -> Self {
        match value {
            WriteLockScope::TargetOrAncestor => Self::TargetOrAncestor,
            WriteLockScope::Descendant => Self::Descendant,
        }
    }
}

impl From<WriteLockScopeWire> for WriteLockScope {
    fn from(value: WriteLockScopeWire) -> Self {
        match value {
            WriteLockScopeWire::TargetOrAncestor => Self::TargetOrAncestor,
            WriteLockScopeWire::Descendant => Self::Descendant,
        }
    }
}
