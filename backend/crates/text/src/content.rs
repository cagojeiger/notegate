//! Pure content metrics: SHA-256, UTF-8 byte length, and logical line count.

/// The derived metrics of a text's content.
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

/// Compute the metrics of plain text content.
pub fn compute(content: &str) -> Metrics {
    Metrics {
        content_sha256: sha256_hex(content.as_bytes()),
        byte_len: content.len(),
        line_count: crate::lines::with_endings(content).count(),
    }
}

/// Hex-encoded SHA-256 of content bytes.
pub fn sha256_hex(content: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(content);
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
