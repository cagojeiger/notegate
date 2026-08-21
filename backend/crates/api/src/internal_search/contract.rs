use chrono::{DateTime, Utc};
use notegate_core::WriteLockScope;
use notegate_model::files::NodeView;
use notegate_model::{NodeKind, TextAtRestEncryption, TextStorageFormat};
use notegate_search::{
    FindMatchMode, FindPage, FindRequest, GrepLineMode, GrepMatchMode, GrepPage, GrepRequest,
    SearchCapacity, SearchError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FindCommand {
    pub caller_account_id: Uuid,
    pub space_id: Uuid,
    pub q: String,
    pub path: Option<String>,
    pub kind: Option<NodeKind>,
    pub match_mode: FindMatchModeWire,
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
            match_mode: request.match_mode.into(),
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
            match_mode: self.match_mode.into(),
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
    pub match_mode: GrepMatchModeWire,
    pub line_mode: GrepLineModeWire,
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
            match_mode: request.match_mode.into(),
            line_mode: request.line_mode.into(),
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
            match_mode: self.match_mode.into(),
            line_mode: self.line_mode.into(),
            include: self.include,
            exclude: self.exclude,
            limit: self.limit,
            cursor: self.cursor,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FindMatchModeWire {
    Contains,
    Regex,
    Glob,
}

impl From<FindMatchMode> for FindMatchModeWire {
    fn from(value: FindMatchMode) -> Self {
        match value {
            FindMatchMode::Contains => Self::Contains,
            FindMatchMode::Regex => Self::Regex,
            FindMatchMode::Glob => Self::Glob,
        }
    }
}

impl From<FindMatchModeWire> for FindMatchMode {
    fn from(value: FindMatchModeWire) -> Self {
        match value {
            FindMatchModeWire::Contains => Self::Contains,
            FindMatchModeWire::Regex => Self::Regex,
            FindMatchModeWire::Glob => Self::Glob,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum GrepMatchModeWire {
    Literal,
    Regex,
}

impl From<GrepMatchMode> for GrepMatchModeWire {
    fn from(value: GrepMatchMode) -> Self {
        match value {
            GrepMatchMode::Literal => Self::Literal,
            GrepMatchMode::Regex => Self::Regex,
        }
    }
}

impl From<GrepMatchModeWire> for GrepMatchMode {
    fn from(value: GrepMatchModeWire) -> Self {
        match value {
            GrepMatchModeWire::Literal => Self::Literal,
            GrepMatchModeWire::Regex => Self::Regex,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum GrepLineModeWire {
    None,
    First,
    All,
}

impl From<GrepLineMode> for GrepLineModeWire {
    fn from(value: GrepLineMode) -> Self {
        match value {
            GrepLineMode::None => Self::None,
            GrepLineMode::First => Self::First,
            GrepLineMode::All => Self::All,
        }
    }
}

impl From<GrepLineModeWire> for GrepLineMode {
    fn from(value: GrepLineModeWire) -> Self {
        match value {
            GrepLineModeWire::None => Self::None,
            GrepLineModeWire::First => Self::First,
            GrepLineModeWire::All => Self::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct FindOutput {
    pub items: Vec<SearchNodeSummary>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

impl From<FindPage> for FindOutput {
    fn from(page: FindPage) -> Self {
        Self {
            items: page
                .items
                .into_iter()
                .map(SearchNodeSummary::from)
                .collect(),
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
    pub node: SearchNodeSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_lines: Vec<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SearchNodeSummary {
    pub path: String,
    pub name: String,
    pub kind: NodeKind,
    pub has_children: bool,
    pub sort_order: i32,
    pub search_enabled: bool,
    pub write_locked: bool,
    pub effective_write_locked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_len: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_storage_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_at_rest_encryption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_metadata: Option<Value>,
}

impl From<NodeView> for SearchNodeSummary {
    fn from(view: NodeView) -> Self {
        let NodeView {
            node,
            path,
            has_children,
            text,
            file,
            write_lock_sources,
        } = view;
        let mut summary = Self {
            path,
            name: node.name,
            kind: node.kind,
            has_children,
            sort_order: node.sort_order,
            search_enabled: node.search_enabled,
            write_locked: node.write_locked,
            effective_write_locked: !write_lock_sources.is_empty(),
            created_at: node.created_at,
            updated_at: node.updated_at,
            content_sha256: None,
            byte_len: None,
            line_count: None,
            text_storage_format: None,
            text_at_rest_encryption: None,
            media_type: None,
            encryption_mode: None,
            original_filename: None,
            encryption_metadata: None,
        };
        if let Some(text) = text {
            summary.content_sha256 = Some(text.content_sha256);
            summary.byte_len = Some(text.byte_len);
            summary.line_count = Some(text.line_count);
            summary.text_storage_format = Some(storage_format_name(text.storage_format));
            summary.text_at_rest_encryption =
                Some(at_rest_encryption_name(text.at_rest_encryption));
        }
        if let Some(file) = file {
            summary.byte_len = Some(file.byte_len);
            summary.media_type = Some(file.media_type);
            summary.encryption_mode = Some(file.encryption_mode.as_str().to_owned());
            summary.original_filename = file.original_filename;
            summary.encryption_metadata = file.encryption_metadata;
        }
        summary
    }
}

fn storage_format_name(value: TextStorageFormat) -> String {
    value.as_str().to_owned()
}

fn at_rest_encryption_name(value: TextAtRestEncryption) -> String {
    value.as_str().to_owned()
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
