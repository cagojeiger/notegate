//! MCP/CLI compact target parsing: `<workspace>:/<absolute-path>`.
//!
//! `target` is syntactic sugar for the structured `workspace` + `path` fields
//! (`docs/spec/mcp/README.md`). Workspace names cannot contain `:` (they match the
//! restricted name grammar), so the target splits on the first `:`; the remainder
//! must be an absolute path inside the workspace.

use notegate_core::validation::validate_workspace_name;

use crate::error::{ServiceError, ServiceResult};

/// A parsed `target`: the workspace name and the absolute path inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub workspace: String,
    pub path: String,
}

/// Parse a `<workspace>:/<path>` target string.
///
/// The workspace segment is validated against the workspace-name grammar; the
/// path segment must start with `/`. Both failures are `400` (`InvalidInput`).
pub fn parse_target(target: &str) -> ServiceResult<Target> {
    let Some((workspace, path)) = target.split_once(':') else {
        return Err(ServiceError::InvalidInput(
            "target must be '<workspace>:/<path>'".to_owned(),
        ));
    };

    validate_workspace_name(workspace)?;

    if !path.starts_with('/') {
        return Err(ServiceError::InvalidInput(
            "target path must start with '/'".to_owned(),
        ));
    }

    Ok(Target {
        workspace: workspace.to_owned(),
        path: path.to_owned(),
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
    fn parses_workspace_and_path() {
        let target = parse_target("personal:/notes/test.md").unwrap();
        assert_eq!(target.workspace, "personal");
        assert_eq!(target.path, "/notes/test.md");
    }

    #[test]
    fn parses_root() {
        let target = parse_target("personal:/").unwrap();
        assert_eq!(target.workspace, "personal");
        assert_eq!(target.path, "/");
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
    fn rejects_invalid_workspace_name() {
        assert!(matches!(
            parse_target(".secret:/notes.md"),
            Err(ServiceError::InvalidInput(_))
        ));
        // An empty workspace segment is invalid.
        assert!(matches!(
            parse_target(":/notes.md"),
            Err(ServiceError::InvalidInput(_))
        ));
    }
}
