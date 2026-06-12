//! Text patch/edit engines for MCP/REST text mutations.
//!
//! String patch is exact by default: each `unique` edit must match exactly once.
//! `first` and `all` are explicit opt-ins for broader replacement. Line edits use
//! 1-based logical line ranges and preserve untouched bytes around edited ranges.

use similar::TextDiff;

use crate::error::ServiceError;

use super::types::{Edit, LineEdit, PatchMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedText {
    pub content: String,
    pub replacements: usize,
}

/// Why a patch/edit failed. Mapped to `400`/`409` by [`PatchError::into_service_error`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    EmptyOldText,
    NoOpEdit,
    NoMatch,
    MultipleMatches,
    CountMismatch { expected: usize, actual: usize },
    OverlappingEdits,
    InvalidLine(String),
}

impl PatchError {
    pub fn into_service_error(self) -> ServiceError {
        match self {
            Self::EmptyOldText => {
                ServiceError::InvalidInput("edit old_text must not be empty".to_owned())
            }
            Self::NoOpEdit => ServiceError::InvalidInput(
                "edit old_text and new_text are identical (no-op)".to_owned(),
            ),
            Self::InvalidLine(message) => ServiceError::InvalidInput(message),
            Self::NoMatch => ServiceError::Conflict(
                "old_text did not match the current text; read it again before patching".to_owned(),
            ),
            Self::MultipleMatches => ServiceError::Conflict(
                "old_text matched multiple times; use mode='all' or include more surrounding context"
                    .to_owned(),
            ),
            Self::CountMismatch { expected, actual } => ServiceError::Conflict(format!(
                "expected_count was {expected}, but current text has {actual} matches"
            )),
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

struct Span<'a> {
    start: usize,
    end: usize,
    new_text: &'a str,
}

/// Apply string replacements against the original content.
pub fn apply_edits(original: &str, edits: &[Edit]) -> Result<AppliedText, PatchError> {
    let mut spans: Vec<Span<'_>> = Vec::new();

    for edit in edits {
        if edit.old_text.is_empty() {
            return Err(PatchError::EmptyOldText);
        }
        if edit.old_text == edit.new_text {
            return Err(PatchError::NoOpEdit);
        }

        let matches: Vec<_> = original.match_indices(edit.old_text.as_str()).collect();
        if matches.is_empty() {
            return Err(PatchError::NoMatch);
        }
        if let Some(expected) = edit.expected_count
            && expected != matches.len()
        {
            return Err(PatchError::CountMismatch {
                expected,
                actual: matches.len(),
            });
        }

        match edit.mode {
            PatchMode::Unique => {
                if matches.len() != 1 {
                    return Err(PatchError::MultipleMatches);
                }
                let matched = matches.first().copied().ok_or(PatchError::NoMatch)?;
                push_match_span(&mut spans, matched, edit.new_text.as_str());
            }
            PatchMode::First => {
                let matched = matches.first().copied().ok_or(PatchError::NoMatch)?;
                push_match_span(&mut spans, matched, edit.new_text.as_str());
            }
            PatchMode::All => {
                for matched in matches {
                    push_match_span(&mut spans, matched, edit.new_text.as_str());
                }
            }
        }
    }

    apply_spans(original, spans)
}

fn push_match_span<'a>(spans: &mut Vec<Span<'a>>, matched: (usize, &'a str), new_text: &'a str) {
    spans.push(Span {
        start: matched.0,
        end: matched.0 + matched.1.len(),
        new_text,
    });
}

/// Apply line-based edits against the original content.
pub fn apply_line_edits(original: &str, edits: &[LineEdit]) -> Result<AppliedText, PatchError> {
    let line_ranges = logical_line_ranges(original);
    let mut spans: Vec<Span<'_>> = Vec::with_capacity(edits.len());

    for edit in edits {
        match edit {
            LineEdit::InsertBefore { line, content } => {
                let index = line_index(*line, line_ranges.len())?;
                let (start, _end) = line_ranges
                    .get(index)
                    .copied()
                    .ok_or_else(|| PatchError::InvalidLine("line is out of range".to_owned()))?;
                spans.push(Span {
                    start,
                    end: start,
                    new_text: content.as_str(),
                });
            }
            LineEdit::InsertAfter { line, content } => {
                let index = line_index(*line, line_ranges.len())?;
                let (_start, end) = line_ranges
                    .get(index)
                    .copied()
                    .ok_or_else(|| PatchError::InvalidLine("line is out of range".to_owned()))?;
                spans.push(Span {
                    start: end,
                    end,
                    new_text: content.as_str(),
                });
            }
            LineEdit::ReplaceLines {
                start_line,
                end_line,
                content,
            } => {
                let (start, end) = line_span(*start_line, *end_line, &line_ranges)?;
                spans.push(Span {
                    start,
                    end,
                    new_text: content.as_str(),
                });
            }
            LineEdit::DeleteLines {
                start_line,
                end_line,
            } => {
                let (start, end) = line_span(*start_line, *end_line, &line_ranges)?;
                spans.push(Span {
                    start,
                    end,
                    new_text: "",
                });
            }
        }
    }

    apply_spans(original, spans)
}

fn apply_spans<'a>(original: &str, mut spans: Vec<Span<'a>>) -> Result<AppliedText, PatchError> {
    spans.sort_by_key(|span| (span.start, span.end));
    let mut prev_end = 0_usize;
    for span in &spans {
        if span.start < prev_end {
            return Err(PatchError::OverlappingEdits);
        }
        prev_end = span.end;
    }

    let mut out = String::with_capacity(original.len());
    let mut cursor = 0_usize;
    for span in &spans {
        out.push_str(&original[cursor..span.start]);
        out.push_str(span.new_text);
        cursor = span.end;
    }
    out.push_str(&original[cursor..]);

    Ok(AppliedText {
        content: out,
        replacements: spans.len(),
    })
}

fn logical_line_ranges(content: &str) -> Vec<(usize, usize)> {
    if content.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut start = 0_usize;
    for (offset, ch) in content.char_indices() {
        if ch == '\n' {
            ranges.push((start, offset + 1));
            start = offset + 1;
        }
    }
    if start < content.len() {
        ranges.push((start, content.len()));
    }
    ranges
}

fn line_index(line: i64, line_count: usize) -> Result<usize, PatchError> {
    if line < 1 || line as usize > line_count {
        return Err(PatchError::InvalidLine(format!(
            "line must be between 1 and {line_count}"
        )));
    }
    Ok(line as usize - 1)
}

fn line_span(
    start_line: i64,
    end_line: i64,
    line_ranges: &[(usize, usize)],
) -> Result<(usize, usize), PatchError> {
    if start_line > end_line {
        return Err(PatchError::InvalidLine(
            "start_line must be less than or equal to end_line".to_owned(),
        ));
    }
    let start = line_index(start_line, line_ranges.len())?;
    let end = line_index(end_line, line_ranges.len())?;
    let start_offset = line_ranges
        .get(start)
        .map(|range| range.0)
        .ok_or_else(|| PatchError::InvalidLine("start_line is out of range".to_owned()))?;
    let end_offset = line_ranges
        .get(end)
        .map(|range| range.1)
        .ok_or_else(|| PatchError::InvalidLine("end_line is out of range".to_owned()))?;
    Ok((start_offset, end_offset))
}

/// Build a small unified diff for a successful patch/edit response.
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
            mode: PatchMode::Unique,
            expected_count: None,
        }
    }

    fn edit_mode(old: &str, new: &str, mode: PatchMode, expected_count: Option<usize>) -> Edit {
        Edit {
            old_text: old.to_owned(),
            new_text: new.to_owned(),
            mode,
            expected_count,
        }
    }

    fn content(result: Result<AppliedText, PatchError>) -> Result<String, PatchError> {
        result.map(|applied| applied.content)
    }

    #[test]
    fn single_exact_match_replaced() {
        let out = content(apply_edits("hello world", &[edit("world", "there")])).unwrap();
        assert_eq!(out, "hello there");
    }

    #[test]
    fn all_matches_replaced_with_expected_count() {
        let out = apply_edits(
            "todo todo done",
            &[edit_mode("todo", "done", PatchMode::All, Some(2))],
        )
        .unwrap();
        assert_eq!(out.content, "done done done");
        assert_eq!(out.replacements, 2);
    }

    #[test]
    fn first_match_replaced() {
        let out = content(apply_edits(
            "todo todo",
            &[edit_mode("todo", "done", PatchMode::First, None)],
        ))
        .unwrap();
        assert_eq!(out, "done todo");
    }

    #[test]
    fn expected_count_mismatch_is_conflict() {
        assert_eq!(
            apply_edits(
                "todo todo",
                &[edit_mode("todo", "done", PatchMode::All, Some(3))]
            ),
            Err(PatchError::CountMismatch {
                expected: 3,
                actual: 2
            })
        );
    }

    #[test]
    fn multiple_non_overlapping_edits_against_original() {
        let out = content(apply_edits("a b c", &[edit("a", "X"), edit("c", "Z")])).unwrap();
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
        assert_eq!(
            apply_edits("abcd", &[edit("abc", "X"), edit("bcd", "Y")]),
            Err(PatchError::OverlappingEdits)
        );
    }

    #[test]
    fn nested_edits_rejected() {
        assert_eq!(
            apply_edits("abcde", &[edit("abcde", "X"), edit("bcd", "Y")]),
            Err(PatchError::OverlappingEdits)
        );
    }

    #[test]
    fn adjacent_edits_allowed() {
        let out = content(apply_edits("abcd", &[edit("ab", "X"), edit("cd", "Y")])).unwrap();
        assert_eq!(out, "XY");
    }

    #[test]
    fn line_endings_are_preserved() {
        let original = "line1\r\nTODO\r\nline3\n";
        let out = content(apply_edits(original, &[edit("TODO", "done")])).unwrap();
        assert_eq!(out, "line1\r\ndone\r\nline3\n");
    }

    #[test]
    fn matching_includes_newlines_exactly() {
        let out = content(apply_edits("a\nb\nc", &[edit("a\nb", "X")])).unwrap();
        assert_eq!(out, "X\nc");
    }

    #[test]
    fn atomic_failure_changes_nothing() {
        let result = apply_edits("hello world", &[edit("hello", "hi"), edit("xyz", "q")]);
        assert_eq!(result, Err(PatchError::NoMatch));
    }

    #[test]
    fn line_insert_replace_delete() {
        let out = apply_line_edits(
            "a\nb\nc\n",
            &[
                LineEdit::InsertAfter {
                    line: 1,
                    content: "x\n".to_owned(),
                },
                LineEdit::ReplaceLines {
                    start_line: 2,
                    end_line: 2,
                    content: "B\n".to_owned(),
                },
                LineEdit::DeleteLines {
                    start_line: 3,
                    end_line: 3,
                },
            ],
        )
        .unwrap();
        assert_eq!(out.content, "a\nx\nB\n");
        assert_eq!(out.replacements, 3);
    }
}
