//! Pure document content metrics: SHA-256, byte length, and line count.
//!
//! These are the values persisted on `documents` and validated against the
//! per-document and workspace caps. `write` and `patch` compute them once here so
//! the validated values are exactly what the store writes.

use super::store::StoredContent;

/// The derived metrics of a document's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metrics {
    /// Hex-encoded SHA-256 of the UTF-8 content.
    pub content_sha256: String,
    /// UTF-8 byte length.
    pub byte_len: usize,
    /// Logical line count (a single trailing `\n` does not add an empty line;
    /// empty content is `0` lines).
    pub line_count: usize,
}

impl Metrics {
    /// Bundle these metrics with their content for the store, converting the
    /// counts to the `i32` columns. Values are validated against the caps before
    /// this is called, so they fit `i32`.
    pub fn into_stored(self, content_md: String) -> StoredContent {
        StoredContent {
            content_md,
            content_sha256: self.content_sha256,
            byte_len: self.byte_len as i32,
            line_count: self.line_count as i32,
        }
    }
}

/// Compute the metrics of document content.
pub fn compute(content: &str) -> Metrics {
    Metrics {
        content_sha256: sha256_hex(content),
        byte_len: content.len(),
        line_count: line_count(content),
    }
}

/// Logical line count: empty content is `0`; otherwise the number of `\n`-joined
/// segments after dropping a single trailing newline.
fn line_count(content: &str) -> usize {
    if content.is_empty() {
        return 0;
    }
    let trimmed = content.strip_suffix('\n').unwrap_or(content);
    trimmed.split('\n').count()
}

/// Hex-encoded SHA-256 of a string's UTF-8 bytes.
fn sha256_hex(content: &str) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(content.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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

    #[test]
    fn empty_content_is_zero_lines() {
        let metrics = compute("");
        assert_eq!(metrics.byte_len, 0);
        assert_eq!(metrics.line_count, 0);
    }

    #[test]
    fn trailing_newline_does_not_add_a_line() {
        assert_eq!(compute("# Note\n").line_count, 1);
        assert_eq!(compute("# Note\n").byte_len, 7);
        assert_eq!(compute("a\nb\n").line_count, 2);
        assert_eq!(compute("a\nb").line_count, 2);
    }

    #[test]
    fn sha256_is_stable_hex() {
        let a = compute("hello").content_sha256;
        assert_eq!(a, compute("hello").content_sha256);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, compute("world").content_sha256);
    }
}
