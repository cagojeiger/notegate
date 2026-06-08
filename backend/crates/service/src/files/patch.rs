//! Exact-match patch engine for `files_patch`.
//!
//! The contract (`docs/spec/files-commands.md`, `docs/spec/mcp/files.md`, and
//! `docs/spec/rest/documents.md`) is exact,
//! not fuzzy: each `old_text` must match the ORIGINAL document exactly once, edits
//! must not be no-ops, ranges must not overlap or nest, and all edits apply
//! atomically against the original (not incrementally). Line endings are preserved
//! because matching and replacement are plain byte-substring operations — there is
//! no normalization.

use similar::TextDiff;

use crate::error::ServiceError;

use super::types::Edit;

/// Why a patch failed. Mapped to `400`/`409` by [`PatchError::into_service_error`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// An `old_text` was empty.
    EmptyOldText,
    /// `old_text` equals `new_text` (a no-op edit).
    NoOpEdit,
    /// An `old_text` matched zero times in the original.
    NoMatch,
    /// An `old_text` matched more than once in the original.
    MultipleMatches,
    /// Two edits target overlapping or nested ranges.
    OverlappingEdits,
}

impl PatchError {
    /// Map to the service error the api turns into an HTTP status.
    ///
    /// Empty/no-op edits are caller-input errors (`400`); zero/multiple matches
    /// and overlapping ranges are state conflicts (`409`) with actionable hints.
    pub fn into_service_error(self) -> ServiceError {
        match self {
            Self::EmptyOldText => {
                ServiceError::InvalidInput("edit old_text must not be empty".to_owned())
            }
            Self::NoOpEdit => ServiceError::InvalidInput(
                "edit old_text and new_text are identical (no-op)".to_owned(),
            ),
            Self::NoMatch => ServiceError::Conflict(
                "old_text did not match the current document; read it again before patching"
                    .to_owned(),
            ),
            Self::MultipleMatches => ServiceError::Conflict(
                "old_text matched multiple times; include more surrounding context".to_owned(),
            ),
            Self::OverlappingEdits => {
                ServiceError::Conflict("edits target overlapping ranges".to_owned())
            }
        }
    }
}

impl From<PatchError> for ServiceError {
    fn from(error: PatchError) -> Self {
        error.into_service_error()
    }
}

/// A resolved edit: the byte range it replaces in the original, and its new text.
struct Span<'a> {
    start: usize,
    end: usize,
    new_text: &'a str,
}

/// Apply `edits` to `original`, returning the new content.
///
/// All matching is against `original` (never against intermediate results). The
/// caller is responsible for the non-empty-`edits` check; this validates each
/// edit, resolves its unique match, rejects overlaps, then applies every span in
/// a single left-to-right pass.
pub fn apply_edits(original: &str, edits: &[Edit]) -> Result<String, PatchError> {
    let mut spans: Vec<Span<'_>> = Vec::with_capacity(edits.len());

    for edit in edits {
        if edit.old_text.is_empty() {
            return Err(PatchError::EmptyOldText);
        }
        if edit.old_text == edit.new_text {
            return Err(PatchError::NoOpEdit);
        }

        let mut matches = original.match_indices(edit.old_text.as_str());
        let Some((start, matched)) = matches.next() else {
            return Err(PatchError::NoMatch);
        };
        if matches.next().is_some() {
            return Err(PatchError::MultipleMatches);
        }

        spans.push(Span {
            start,
            end: start + matched.len(),
            new_text: edit.new_text.as_str(),
        });
    }

    // Order by start offset and reject overlapping or nested ranges: each span
    // must begin at or after the previous span's end.
    spans.sort_by_key(|span| span.start);
    let mut prev_end = 0_usize;
    for span in &spans {
        if span.start < prev_end {
            return Err(PatchError::OverlappingEdits);
        }
        prev_end = span.end;
    }

    // Apply every span against the original in one pass.
    let mut out = String::with_capacity(original.len());
    let mut cursor = 0_usize;
    for span in &spans {
        out.push_str(&original[cursor..span.start]);
        out.push_str(span.new_text);
        cursor = span.end;
    }
    out.push_str(&original[cursor..]);

    Ok(out)
}

/// Build a small unified diff for a successful patch response.
///
/// This is presentation-only: patch application remains the exact-match engine
/// above, while `similar` only formats the before/after text for clients.
pub fn unified_diff(before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(3)
        .header("before.md", "after.md")
        .to_string()
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

    fn edit(old: &str, new: &str) -> Edit {
        Edit {
            old_text: old.to_owned(),
            new_text: new.to_owned(),
        }
    }

    #[test]
    fn single_exact_match_replaced() {
        let out = apply_edits("hello world", &[edit("world", "there")]).unwrap();
        assert_eq!(out, "hello there");
    }

    #[test]
    fn multiple_non_overlapping_edits_against_original() {
        // Both matched against the original, applied atomically.
        let out = apply_edits("a b c", &[edit("a", "X"), edit("c", "Z")]).unwrap();
        assert_eq!(out, "X b Z");
    }

    #[test]
    fn zero_matches_is_conflict() {
        assert_eq!(
            apply_edits("hello", &[edit("missing", "x")]),
            Err(PatchError::NoMatch)
        );
    }

    #[test]
    fn multiple_matches_is_conflict() {
        assert_eq!(
            apply_edits("aa", &[edit("a", "b")]),
            Err(PatchError::MultipleMatches)
        );
    }

    #[test]
    fn empty_old_text_rejected() {
        assert_eq!(
            apply_edits("hello", &[edit("", "x")]),
            Err(PatchError::EmptyOldText)
        );
    }

    #[test]
    fn no_op_edit_rejected() {
        assert_eq!(
            apply_edits("hello", &[edit("hello", "hello")]),
            Err(PatchError::NoOpEdit)
        );
    }

    #[test]
    fn overlapping_edits_rejected() {
        // "abc" and "bcd" overlap on "bc" in "abcd".
        assert_eq!(
            apply_edits("abcd", &[edit("abc", "X"), edit("bcd", "Y")]),
            Err(PatchError::OverlappingEdits)
        );
    }

    #[test]
    fn nested_edits_rejected() {
        // "bcd" is nested within "abcde".
        assert_eq!(
            apply_edits("abcde", &[edit("abcde", "X"), edit("bcd", "Y")]),
            Err(PatchError::OverlappingEdits)
        );
    }

    #[test]
    fn adjacent_edits_allowed() {
        // "ab" ends exactly where "cd" begins — not overlapping.
        let out = apply_edits("abcd", &[edit("ab", "X"), edit("cd", "Y")]).unwrap();
        assert_eq!(out, "XY");
    }

    #[test]
    fn line_endings_are_preserved() {
        // CRLF in the surrounding text is untouched; only the exact match changes.
        let original = "line1\r\nTODO\r\nline3\n";
        let out = apply_edits(original, &[edit("TODO", "done")]).unwrap();
        assert_eq!(out, "line1\r\ndone\r\nline3\n");
    }

    #[test]
    fn matching_includes_newlines_exactly() {
        // old_text spanning a newline matches the exact bytes.
        let out = apply_edits("a\nb\nc", &[edit("a\nb", "X")]).unwrap();
        assert_eq!(out, "X\nc");
    }

    #[test]
    fn atomic_failure_changes_nothing() {
        // Second edit has no match → the whole patch fails; nothing is returned.
        let result = apply_edits("hello world", &[edit("hello", "hi"), edit("xyz", "q")]);
        assert_eq!(result, Err(PatchError::NoMatch));
    }
}
