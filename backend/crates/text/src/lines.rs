//! Shared logical-line rules for reading, editing, metrics, and search.
//!
//! Empty text has no lines. An LF terminates its line without introducing an
//! extra trailing line. CR bytes are preserved, including the CR in CRLF.

use std::ops::Range;

/// Logical lines including their original line endings.
pub fn with_endings(content: &str) -> impl Iterator<Item = &str> {
    content.split_inclusive('\n')
}

/// Logical lines without the terminating LF, preserving any CR for matching.
pub fn logical_lines(content: &str) -> impl Iterator<Item = &str> {
    with_endings(content).map(|line| line.strip_suffix('\n').unwrap_or(line))
}

/// UTF-8 byte ranges of logical lines, including their original line endings.
pub fn line_ranges(content: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    with_endings(content).scan(0, |offset, line| {
        let start = *offset;
        *offset += line.len();
        Some(start..*offset)
    })
}
