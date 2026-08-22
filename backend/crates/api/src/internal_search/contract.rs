use axum::http::StatusCode;
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
pub(super) struct InternalSearchRequest<T> {
    pub(super) deadline_unix_ms: i64,
    pub(super) command: T,
}

#[derive(Debug, Serialize, Deserialize)]
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
    DeadlineExceeded,
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

    /// Stable machine-readable code exposed by the public REST/MCP contracts.
    pub(super) const fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "not_found",
            Self::InvalidInput { .. } => "invalid_input",
            Self::Forbidden { .. } => "forbidden",
            Self::Conflict { .. } => "conflict",
            Self::WriteLocked { scope } => match scope {
                WriteLockScopeWire::TargetOrAncestor => "node_write_locked",
                WriteLockScopeWire::Descendant => "subtree_write_locked",
            },
            Self::UsageRecalculationInProgress { .. } => "usage_recalculation_in_progress",
            Self::Busy { .. } => "search_busy",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::Internal => "internal_error",
        }
    }

    /// Canonical HTTP status for this private wire error.
    pub(super) const fn status(&self) -> StatusCode {
        match self {
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::InvalidInput { .. } => StatusCode::BAD_REQUEST,
            Self::Forbidden { .. } => StatusCode::FORBIDDEN,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::WriteLocked { .. } => StatusCode::LOCKED,
            Self::UsageRecalculationInProgress { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Self::Busy { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn ids() -> (Uuid, Uuid) {
        (Uuid::new_v4(), Uuid::new_v4())
    }

    #[test]
    fn commands_ignore_future_optional_fields_but_keep_required_fields_strict() {
        let (caller_account_id, space_id) = ids();
        let find = json!({
            "caller_account_id": caller_account_id,
            "space_id": space_id,
            "q": "README",
            "path": "/docs",
            "kind": null,
            "match_mode": "contains",
            "include": [],
            "exclude": [],
            "limit": 20,
            "cursor": null,
            "future_optional_field": true,
        });
        let grep = json!({
            "caller_account_id": caller_account_id,
            "space_id": space_id,
            "q": "needle",
            "path": null,
            "match_mode": "literal",
            "line_mode": "none",
            "include": [],
            "exclude": [],
            "limit": 20,
            "cursor": null,
            "future_optional_field": {"value": 1},
        });

        assert!(serde_json::from_value::<FindCommand>(find.clone()).is_ok());
        assert!(serde_json::from_value::<GrepCommand>(grep).is_ok());

        let mut missing_required = find;
        assert!(
            missing_required
                .as_object_mut()
                .is_some_and(|command| command.remove("q").is_some())
        );
        assert!(serde_json::from_value::<FindCommand>(missing_required).is_err());
    }

    #[test]
    fn request_envelope_requires_a_deadline_and_command() {
        let (caller_account_id, space_id) = ids();
        let input = json!({
            "deadline_unix_ms": 1_000,
            "command": {
                "caller_account_id": caller_account_id,
                "space_id": space_id,
                "q": "README",
                "path": null,
                "kind": null,
                "match_mode": "contains",
                "include": [],
                "exclude": [],
                "limit": 20,
                "cursor": null,
            },
            "future_optional_field": true,
        });

        assert!(
            serde_json::from_value::<InternalSearchRequest<FindCommand>>(input.clone()).is_ok()
        );
        let mut missing_deadline = input;
        assert!(
            missing_deadline
                .as_object_mut()
                .is_some_and(|request| request.remove("deadline_unix_ms").is_some())
        );
        assert!(
            serde_json::from_value::<InternalSearchRequest<FindCommand>>(missing_deadline).is_err()
        );
    }

    #[test]
    fn outputs_ignore_fields_added_by_a_newer_search_process() -> serde_json::Result<()> {
        let output = json!({
            "items": [],
            "limit": 20,
            "has_more": false,
            "next_cursor": null,
            "execution_ms": 4,
        });

        let parsed: FindOutput = serde_json::from_value(output)?;
        assert!(parsed.items.is_empty());
        assert_eq!(parsed.limit, 20);

        let serialized = serde_json::to_value(parsed)?;
        assert_eq!(serialized.get("execution_ms"), None::<&Value>);
        Ok(())
    }

    #[test]
    fn private_errors_share_public_codes_and_canonical_statuses() {
        let cases = [
            (
                InternalSearchError::InvalidInput {
                    message: "bad".to_owned(),
                },
                "invalid_input",
                StatusCode::BAD_REQUEST,
            ),
            (
                InternalSearchError::WriteLocked {
                    scope: WriteLockScopeWire::Descendant,
                },
                "subtree_write_locked",
                StatusCode::LOCKED,
            ),
            (
                InternalSearchError::UsageRecalculationInProgress {
                    retry_after_seconds: 5,
                },
                "usage_recalculation_in_progress",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                InternalSearchError::busy(SearchCapacity::Grep),
                "search_busy",
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (
                InternalSearchError::DeadlineExceeded,
                "deadline_exceeded",
                StatusCode::GATEWAY_TIMEOUT,
            ),
        ];

        for (error, code, status) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(error.status(), status);
        }
    }
}
