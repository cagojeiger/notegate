//! Workspace search: `find` (node name metadata) and `grep` (content).
//!
//! The service owns authorization, limit clamping, opaque cursors, and
//! service-side grep line splitting. The two query implementations live in the
//! [`find`] and [`grep`] submodules; shared types, the store trait, the role
//! gate, and query validation live here.

use std::future::Future;

use notegate_core::Result as CoreResult;
use notegate_core::limits;
use notegate_model::{Node, NodeKind, Role};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::files::NodeView;
use crate::files::policy::{self, FileCommand};

mod find;
mod grep;

/// `find` request.
#[derive(Debug, Clone)]
pub struct FindRequest {
    pub q: String,
    pub path: Option<String>,
    pub kind: Option<NodeKind>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// `find` keyset cursor over `(name, id)`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FindCursor {
    pub name: String,
    pub id: Uuid,
}

/// `grep` request.
#[derive(Debug, Clone)]
pub struct GrepRequest {
    pub q: String,
    pub path: Option<String>,
    pub context: Option<i64>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// `grep` keyset cursor over `(updated_at, node_id)` plus an intra-document
/// match offset.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GrepCursor {
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub node_id: Uuid,
    pub match_offset: i64,
}

/// One grep match with context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepMatch {
    pub node_id: Uuid,
    pub path: String,
    pub line_no: i64,
    pub line: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// A find result page.
#[derive(Debug, Clone)]
pub struct FindPage {
    pub items: Vec<NodeView>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// A grep result page.
#[derive(Debug, Clone)]
pub struct GrepPage {
    pub items: Vec<GrepMatch>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// A candidate document for grep line-splitting.
#[derive(Debug, Clone)]
pub struct GrepCandidate {
    pub node_id: Uuid,
    pub path: String,
    pub content_md: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Persistence for search queries.
pub trait SearchStore: Clone + Send + Sync + 'static {
    /// The caller's live role in a workspace, or `None` if no live grant. Used to
    /// authorize search the same way file commands are (no role ⇒ `404`).
    fn role_for(
        &self,
        workspace_id: Uuid,
        account_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<Role>>> + Send;

    /// Find nodes by name within a workspace (keyset). Each row carries the
    /// node, its derived display path, and whether it has any live children, so
    /// the service can assemble a [`NodeView`] without an extra per-row query.
    fn find_nodes(
        &self,
        workspace_id: Uuid,
        q: &str,
        scope: Option<&str>,
        kind: Option<NodeKind>,
        limit: i64,
        cursor: Option<&FindCursor>,
    ) -> impl Future<Output = CoreResult<Vec<(Node, String, bool)>>> + Send;

    /// Fetch grep candidate documents (content match + scope), keyset by
    /// `(updated_at, node_id)`.
    fn grep_candidates(
        &self,
        workspace_id: Uuid,
        q: &str,
        scope: Option<&str>,
        limit: i64,
        cursor: Option<&GrepCursor>,
    ) -> impl Future<Output = CoreResult<Vec<GrepCandidate>>> + Send;
}

/// Search service. The `find`/`grep` query methods are implemented in the
/// [`find`] and [`grep`] submodules.
#[derive(Debug, Clone)]
pub struct SearchService<S> {
    store: S,
}

impl<S> SearchService<S>
where
    S: SearchStore,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Resolve the caller's role (no role ⇒ `404`) and gate by command
    /// (lesser role ⇒ `403`). Mirrors the file service's authorization.
    async fn authorize(
        &self,
        workspace_id: Uuid,
        account_id: Uuid,
        command: FileCommand,
    ) -> ServiceResult<Role> {
        let role = self
            .store
            .role_for(workspace_id, account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("workspace not found".to_owned()))?;
        policy::require(role, command)?;
        Ok(role)
    }
}

/// Reject empty, multi-line, or very long search strings before they can become
/// broad or expensive `ILIKE` scans.
fn validate_query(q: &str) -> ServiceResult<&str> {
    let trimmed = q.trim();
    if trimmed.is_empty() {
        return Err(ServiceError::InvalidInput(
            "search query cannot be empty".to_owned(),
        ));
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(ServiceError::InvalidInput(
            "search query must be a single line".to_owned(),
        ));
    }
    if trimmed.chars().count() > limits::SEARCH_QUERY_MAX_CHARS {
        return Err(ServiceError::InvalidInput(format!(
            "search query must be at most {} characters",
            limits::SEARCH_QUERY_MAX_CHARS
        )));
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use super::*;
    use crate::cursor;
    use chrono::Utc;

    /// The `find` cursor round-trips through the shared opaque codec, preserving
    /// its exact `(name, id)` tuple.
    #[test]
    fn find_cursor_round_trips() {
        let value = FindCursor {
            name: "note.md".to_owned(),
            id: Uuid::new_v4(),
        };
        let encoded = cursor::encode(&value).unwrap();
        let decoded: FindCursor = cursor::decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    /// The `grep` cursor round-trips, preserving `(updated_at, node_id,
    /// match_offset)` exactly — including the intra-document offset.
    #[test]
    fn grep_cursor_round_trips() {
        let value = GrepCursor {
            updated_at: Utc::now(),
            node_id: Uuid::new_v4(),
            match_offset: 7,
        };
        let encoded = cursor::encode(&value).unwrap();
        let decoded: GrepCursor = cursor::decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    /// A garbage cursor fails to decode for both cursor types.
    #[test]
    fn garbage_cursor_fails_to_decode() {
        assert!(cursor::decode::<FindCursor>("!!!not-base64!!!").is_err());
        assert!(cursor::decode::<GrepCursor>("not-a-cursor").is_err());
    }

    #[test]
    fn invalid_queries_are_rejected() {
        assert!(matches!(
            validate_query("   "),
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_query("alpha\nbeta"),
            Err(ServiceError::InvalidInput(_))
        ));
        let too_long = "x".repeat(limits::SEARCH_QUERY_MAX_CHARS + 1);
        assert!(matches!(
            validate_query(&too_long),
            Err(ServiceError::InvalidInput(_))
        ));
        assert_eq!(validate_query("  note  ").unwrap(), "note");
    }
}
