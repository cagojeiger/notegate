//! MCP/CLI compact target parsing: `<space>:/<absolute-path>`.
//!
//! `target` is syntactic sugar for the structured `space` + `path` fields
//! (`docs/spec/mcp/README.md`). Space names cannot contain `:`, so the target
//! splits on the first `:`; the remainder must be an absolute path inside the
//! space.

use notegate_core::validation::{normalize_path, validate_space_name};

use crate::error::{ServiceError, ServiceResult};

/// A parsed `target`: the space name and the absolute path inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub space: String,
    pub path: String,
}

/// Parse a `<space>:/<path>` target string.
///
/// The space segment is validated against the space-name grammar; the
/// path segment must start with `/`. Both failures are `400` (`InvalidInput`).
pub fn parse_target(target: &str) -> ServiceResult<Target> {
    let Some((space, path)) = target.split_once(':') else {
        return Err(ServiceError::InvalidInput(
            "target must be '<space>:/<path>'".to_owned(),
        ));
    };

    validate_space_name(space)?;

    let path = normalize_path(path)?;

    Ok(Target {
        space: space.to_owned(),
        path,
    })
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
    fn parses_space_and_path() {
        let target = parse_target("personal:/notes/test.md").unwrap();
        assert_eq!(target.space, "personal");
        assert_eq!(target.path, "/notes/test.md");
    }

    #[test]
    fn parses_root() {
        let target = parse_target("personal:/").unwrap();
        assert_eq!(target.space, "personal");
        assert_eq!(target.path, "/");
    }

    #[test]
    fn normalizes_path() {
        let target = parse_target("personal:/notes//test.md/").unwrap();
        assert_eq!(target.path, "/notes/test.md");
    }

    #[test]
    fn rejects_missing_colon() {
        assert!(matches!(
            parse_target("personal/notes.md"),
            Err(ServiceError::InvalidInput(_))
        ));
    }

    #[test]
    fn rejects_non_absolute_path() {
        assert!(matches!(
            parse_target("personal:notes.md"),
            Err(ServiceError::InvalidInput(_))
        ));
    }

    #[test]
    fn rejects_invalid_space_name() {
        assert!(matches!(
            parse_target("bad/name:/notes.md"),
            Err(ServiceError::InvalidInput(_))
        ));
        // An empty space segment is invalid.
        assert!(matches!(
            parse_target(":/notes.md"),
            Err(ServiceError::InvalidInput(_))
        ));
    }
}
