//! `grep`: text-content search. Candidate texts are fetched keyset by
//! `(updated_at DESC, node_id)`; line-splitting and context assembly happen here
//! in the service so a match carries its 1-based `line_no` and context lines.

use notegate_core::limits;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::files::policy::FileCommand;
use crate::files::validation;
use crate::pagination::clamp_limit;

use super::{
    GrepCandidate, GrepCursor, GrepMatch, GrepPage, GrepRequest, SearchService, validate_query,
};

impl SearchService {
    /// Grep text content: fetch candidate texts (content match + scope),
    /// then split each candidate's content into lines here in the service so a
    /// match carries its 1-based `line_no` and `context` lines before/after.
    ///
    /// Keyset-paginated by `(updated_at DESC, node_id)` plus an intra-text
    /// `match_offset`, so a single text with more matches than the page limit
    /// resumes exactly where the previous page stopped. Authorization mirrors file
    /// reads (`grep` requires `viewer`; no role ⇒ `404`). The limit is clamped to
    /// `1..=GREP_MAX_LIMIT` (default `GREP_DEFAULT_LIMIT`); context is clamped to
    /// `0..=GREP_MAX_CONTEXT` (default `GREP_DEFAULT_CONTEXT`). A malformed cursor
    /// is a clean `400`-class [`ServiceError::InvalidInput`].
    pub async fn grep(
        &self,
        caller_account_id: uuid::Uuid,
        space_id: uuid::Uuid,
        request: GrepRequest,
    ) -> ServiceResult<GrepPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Grep)
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
        // candidate texts in keyset order, line-splitting each; a text
        // may contribute many matches (and a previous page may have stopped mid
        // text, encoded as the cursor's `match_offset`).
        let mut matches: Vec<GrepMatch> = Vec::with_capacity(limit as usize + 1);
        let want = limit + 1;

        // Page through candidate texts until we have enough matches or the
        // candidates are exhausted. Each text is bounded (≤2000 lines) and a
        // space is bounded (≤5000 texts), so this loop is bounded. The
        // text keyset is INCLUSIVE of `cursor.node_id`, so each batch re-reads
        // the cursor's text first and skips its already-emitted matches via
        // `cursor.match_offset`.
        'outer: loop {
            let candidates = self
                .store
                .grep_candidates(space_id, q, scope_path.as_deref(), want, cursor.as_ref())
                .await?;

            if candidates.is_empty() {
                break;
            }
            let batch_len = candidates.len();

            // Skip the matches already emitted from the batch's first text
            // (the cursor's text); later texts in the batch start at 0.
            let mut skip_in_first = cursor.as_ref().map(|c| c.match_offset).unwrap_or(0);

            for candidate in candidates {
                let doc_matches = grep_text(&candidate, q, context);

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

                // Text fully consumed. Record the cursor at this text with
                // its total emitted count; because the next batch's keyset is
                // inclusive of this node_id, the following batch will re-read it
                // and skip exactly `emitted_in_doc` matches (i.e. skip it whole).
                cursor = Some(GrepCursor {
                    updated_at: candidate.updated_at,
                    node_id: candidate.node_id,
                    match_offset: emitted_in_doc as i64,
                });
            }

            // Fetched fewer texts than requested ⇒ candidates exhausted.
            if (batch_len as i64) < want {
                break;
            }
        }

        let has_more = matches.len() as i64 > limit;
        matches.truncate(limit as usize);

        // `next_cursor` is only meaningful when more results remain; the loop
        // already set `cursor` to the resume point when it broke early.
        let next_cursor = if has_more {
            cursor
                .map(|cursor| cursor::encode(&cursor))
                .transpose()
                .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
        } else {
            None
        };

        Ok(GrepPage {
            items: matches,
            limit,
            has_more,
            next_cursor,
        })
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

/// Split a candidate text into logical lines and emit one [`GrepMatch`] per
/// line that contains `q` (case-insensitive substring, matching the SQL ILIKE),
/// each carrying up to `context` lines before and after. Lines are 1-based.
fn grep_text(candidate: &GrepCandidate, q: &str, context: i64) -> Vec<GrepMatch> {
    let lines = split_lines(&candidate.content);
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
/// text ending in `\n` is not counted as a trailing empty line. Mirrors the
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
    use uuid::Uuid;

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
            content: content.to_owned(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn grep_text_reports_line_no_and_context() {
        let doc = candidate("l1\nl2\nhit\nl4\nl5\n");
        let matches = grep_text(&doc, "hit", 1);
        assert_eq!(matches.len(), 1);
        let m = &matches[0];
        assert_eq!(m.line_no, 3, "1-based line number");
        assert_eq!(m.line, "hit");
        assert_eq!(m.before, vec!["l2".to_owned()]);
        assert_eq!(m.after, vec!["l4".to_owned()]);
    }

    #[test]
    fn grep_text_context_is_bounded_at_text_edges() {
        // Match on the first line: no `before`, `after` bounded by available lines.
        let doc = candidate("hit\nl2\n");
        let matches = grep_text(&doc, "hit", 5);
        assert_eq!(matches.len(), 1);
        assert!(
            matches[0].before.is_empty(),
            "no lines before the first line"
        );
        assert_eq!(matches[0].after, vec!["l2".to_owned()]);
    }

    #[test]
    fn grep_text_matches_case_insensitively() {
        let doc = candidate("Alpha BETA\n");
        assert_eq!(
            grep_text(&doc, "beta", 0).len(),
            1,
            "ILIKE-style case folding"
        );
        assert_eq!(grep_text(&doc, "ALPHA", 0).len(), 1);
    }
}
