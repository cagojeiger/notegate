//! Workspace search: `find` (node name metadata) and `grep` (content).
//!
//! The service owns authorization, limit clamping, opaque cursors, and
//! service-side grep line splitting.

use std::future::Future;

use notegate_core::Result as CoreResult;
use notegate_core::limits;
use notegate_model::{Node, NodeKind, Role};
use uuid::Uuid;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::files::NodeView;
use crate::files::policy::{self, FileCommand};
use crate::files::validation;

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
    pub next_cursor: Option<FindCursor>,
}

/// A grep result page.
#[derive(Debug, Clone)]
pub struct GrepPage {
    pub items: Vec<GrepMatch>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<GrepCursor>,
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

/// Search service.
#[derive(Debug, Clone)]
pub struct SearchService<S> {
    #[allow(dead_code)]
    store: S,
}

impl<S> SearchService<S>
where
    S: SearchStore,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Find nodes by name, optionally filtered by `kind` and scoped to a path's
    /// subtree. Keyset-paginated by `(name, id)`.
    ///
    /// Authorization mirrors file reads: the caller's live workspace role is
    /// resolved first (no role ⇒ `404`, which hides the workspace); `find`
    /// requires `viewer`. The limit is clamped to `1..=FIND_MAX_LIMIT` (default
    /// `FIND_DEFAULT_LIMIT`); a malformed cursor is a clean `400`-class
    /// [`ServiceError::InvalidInput`].
    pub async fn find(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        request: FindRequest,
    ) -> ServiceResult<FindPage> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Find)
            .await?;
        let q = validate_query(&request.q)?;
        let limit = clamp_limit(
            request.limit,
            limits::FIND_DEFAULT_LIMIT,
            limits::FIND_MAX_LIMIT,
        );

        // Decode the opaque cursor (garbage/tampered → InvalidInput → 400).
        let cursor: Option<FindCursor> = match request.cursor.as_deref() {
            None => None,
            Some(raw) => Some(cursor::decode(raw)?),
        };

        let scope_path = request
            .path
            .as_deref()
            .map(validation::normalize_path)
            .transpose()?;

        // Fetch `limit + 1` to detect a next page without a second query.
        let rows = self
            .store
            .find_nodes(
                workspace_id,
                q,
                scope_path.as_deref(),
                request.kind,
                limit + 1,
                cursor.as_ref(),
            )
            .await?;

        let has_more = rows.len() as i64 > limit;
        let mut rows = rows;
        rows.truncate(limit as usize);

        // The next cursor is the LAST returned row's `(name, id)` keyset.
        let next_cursor = if has_more {
            rows.last().map(|(node, _path, _has_children)| FindCursor {
                name: node.name.clone(),
                id: node.id,
            })
        } else {
            None
        };

        let items = rows
            .into_iter()
            .map(|(node, path, has_children)| NodeView {
                node,
                path,
                has_children,
                document: None,
            })
            .collect();

        Ok(FindPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    /// Grep document content: fetch candidate documents (content match + scope),
    /// then split each candidate's content into lines here in the service so a
    /// match carries its 1-based `line_no` and `context` lines before/after.
    ///
    /// Keyset-paginated by `(updated_at DESC, node_id)` plus an intra-document
    /// `match_offset`, so a single document with more matches than the page limit
    /// resumes exactly where the previous page stopped. Authorization mirrors file
    /// reads (`grep` requires `viewer`; no role ⇒ `404`). The limit is clamped to
    /// `1..=GREP_MAX_LIMIT` (default `GREP_DEFAULT_LIMIT`); context is clamped to
    /// `0..=GREP_MAX_CONTEXT` (default `GREP_DEFAULT_CONTEXT`). A malformed cursor
    /// is a clean `400`-class [`ServiceError::InvalidInput`].
    pub async fn grep(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        request: GrepRequest,
    ) -> ServiceResult<GrepPage> {
        self.authorize(workspace_id, caller_account_id, FileCommand::Grep)
            .await?;
        let q = validate_query(&request.q)?;
        let limit = clamp_limit(
            request.limit,
            limits::GREP_DEFAULT_LIMIT,
            limits::GREP_MAX_LIMIT,
        );
        let context = clamp_context(request.context);

        let mut cursor: Option<GrepCursor> = match request.cursor.as_deref() {
            None => None,
            Some(raw) => Some(cursor::decode(raw)?),
        };
        let scope_path = request
            .path
            .as_deref()
            .map(validation::normalize_path)
            .transpose()?;

        // Accumulate up to `limit + 1` matches to detect a next page. Iterate
        // candidate documents in keyset order, line-splitting each; a document
        // may contribute many matches (and a previous page may have stopped mid
        // document, encoded as the cursor's `match_offset`).
        let mut matches: Vec<GrepMatch> = Vec::with_capacity(limit as usize + 1);
        let want = limit + 1;

        // Page through candidate documents until we have enough matches or the
        // candidates are exhausted. Each document is bounded (≤2000 lines) and a
        // workspace is bounded (≤5000 documents), so this loop is bounded. The
        // document keyset is INCLUSIVE of `cursor.node_id`, so each batch re-reads
        // the cursor's document first and skips its already-emitted matches via
        // `cursor.match_offset`.
        'outer: loop {
            let candidates = self
                .store
                .grep_candidates(
                    workspace_id,
                    q,
                    scope_path.as_deref(),
                    want,
                    cursor.as_ref(),
                )
                .await?;

            if candidates.is_empty() {
                break;
            }
            let batch_len = candidates.len();

            // Skip the matches already emitted from the batch's first document
            // (the cursor's document); later documents in the batch start at 0.
            let mut skip_in_first = cursor.as_ref().map(|c| c.match_offset).unwrap_or(0);

            for candidate in candidates {
                let doc_matches = grep_document(&candidate, q, context);

                let start = std::mem::take(&mut skip_in_first).max(0) as usize;
                let mut emitted_in_doc = start;
                for found in doc_matches.into_iter().skip(start) {
                    matches.push(found);
                    emitted_in_doc += 1;
                    if matches.len() as i64 >= want {
                        // `want = limit + 1`: this last match is the lookahead
                        // sentinel that proves `has_more` and is truncated off the
                        // returned page. The next page must therefore RESUME at it,
                        // so the resume offset excludes it (`emitted_in_doc - 1`).
                        cursor = Some(GrepCursor {
                            updated_at: candidate.updated_at,
                            node_id: candidate.node_id,
                            match_offset: (emitted_in_doc - 1) as i64,
                        });
                        break 'outer;
                    }
                }

                // Document fully consumed. Record the cursor at this document with
                // its total emitted count; because the next batch's keyset is
                // inclusive of this node_id, the following batch will re-read it
                // and skip exactly `emitted_in_doc` matches (i.e. skip it whole).
                cursor = Some(GrepCursor {
                    updated_at: candidate.updated_at,
                    node_id: candidate.node_id,
                    match_offset: emitted_in_doc as i64,
                });
            }

            // Fetched fewer documents than requested ⇒ candidates exhausted.
            if (batch_len as i64) < want {
                break;
            }
        }

        let has_more = matches.len() as i64 > limit;
        matches.truncate(limit as usize);

        // `next_cursor` is only meaningful when more results remain; the loop
        // already set `cursor` to the resume point when it broke early.
        let next_cursor = if has_more { cursor } else { None };

        Ok(GrepPage {
            items: matches,
            limit,
            has_more,
            next_cursor,
        })
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

/// Clamp a requested page limit to `1..=max`, defaulting to `default` when absent.
fn clamp_limit(limit: Option<i64>, default: i64, max: i64) -> i64 {
    match limit {
        None => default,
        Some(value) if value < 1 => 1,
        Some(value) => value.min(max),
    }
}

/// Clamp grep context lines to `0..=GREP_MAX_CONTEXT`, defaulting to
/// `GREP_DEFAULT_CONTEXT` when absent.
fn clamp_context(context: Option<i64>) -> i64 {
    match context {
        None => limits::GREP_DEFAULT_CONTEXT,
        Some(value) if value < 0 => 0,
        Some(value) => value.min(limits::GREP_MAX_CONTEXT),
    }
}

/// Split a candidate document into logical lines and emit one [`GrepMatch`] per
/// line that contains `q` (case-insensitive substring, matching the SQL ILIKE),
/// each carrying up to `context` lines before and after. Lines are 1-based.
fn grep_document(candidate: &GrepCandidate, q: &str, context: i64) -> Vec<GrepMatch> {
    let lines = split_lines(&candidate.content_md);
    let needle = q.to_lowercase();
    let context = context.max(0) as usize;

    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !line.to_lowercase().contains(&needle) {
            continue;
        }
        let before_start = index.saturating_sub(context);
        let before = lines
            .get(before_start..index)
            .unwrap_or(&[])
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let after_end = (index + 1 + context).min(lines.len());
        let after = lines
            .get(index + 1..after_end)
            .unwrap_or(&[])
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        out.push(GrepMatch {
            node_id: candidate.node_id,
            path: candidate.path.clone(),
            line_no: (index + 1) as i64,
            line: (*line).to_owned(),
            before,
            after,
        });
    }
    out
}

/// Split content into logical lines, dropping a single trailing newline so a
/// document ending in `\n` is not counted as a trailing empty line. Mirrors the
/// line-count semantics used by the files service.
fn split_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    let trimmed = content.strip_suffix('\n').unwrap_or(content);
    trimmed.split('\n').collect()
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

    #[test]
    fn limit_is_clamped_to_range() {
        assert_eq!(clamp_limit(None, 50, 100), 50, "absent → default");
        assert_eq!(clamp_limit(Some(0), 50, 100), 1, "below 1 → 1");
        assert_eq!(clamp_limit(Some(250), 50, 100), 100, "above max → max");
        assert_eq!(clamp_limit(Some(30), 50, 100), 30, "in range → as-is");
    }

    #[test]
    fn context_is_clamped_to_range() {
        assert_eq!(
            clamp_context(None),
            limits::GREP_DEFAULT_CONTEXT,
            "absent → default"
        );
        assert_eq!(clamp_context(Some(-3)), 0, "negative → 0");
        assert_eq!(
            clamp_context(Some(100)),
            limits::GREP_MAX_CONTEXT,
            "above max → max"
        );
        assert_eq!(clamp_context(Some(3)), 3, "in range → as-is");
    }

    fn candidate(content: &str) -> GrepCandidate {
        GrepCandidate {
            node_id: Uuid::new_v4(),
            path: "/note.md".to_owned(),
            content_md: content.to_owned(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn grep_document_reports_line_no_and_context() {
        let doc = candidate("l1\nl2\nhit\nl4\nl5\n");
        let matches = grep_document(&doc, "hit", 1);
        assert_eq!(matches.len(), 1);
        let m = &matches[0];
        assert_eq!(m.line_no, 3, "1-based line number");
        assert_eq!(m.line, "hit");
        assert_eq!(m.before, vec!["l2".to_owned()]);
        assert_eq!(m.after, vec!["l4".to_owned()]);
    }

    #[test]
    fn grep_document_context_is_bounded_at_document_edges() {
        // Match on the first line: no `before`, `after` bounded by available lines.
        let doc = candidate("hit\nl2\n");
        let matches = grep_document(&doc, "hit", 5);
        assert_eq!(matches.len(), 1);
        assert!(
            matches[0].before.is_empty(),
            "no lines before the first line"
        );
        assert_eq!(matches[0].after, vec!["l2".to_owned()]);
    }

    #[test]
    fn grep_document_matches_case_insensitively() {
        let doc = candidate("Alpha BETA\n");
        assert_eq!(
            grep_document(&doc, "beta", 0).len(),
            1,
            "ILIKE-style case folding"
        );
        assert_eq!(grep_document(&doc, "ALPHA", 0).len(), 1);
    }
}
