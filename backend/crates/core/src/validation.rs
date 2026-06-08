//! Shared name and path validation.
//!
//! The regex patterns here are the single source of truth for workspace and
//! node names and must stay aligned with the database `CHECK` constraints in
//! `backend/crates/db/migrations/0001_init.sql`.

use std::sync::LazyLock;

use regex::Regex;

use crate::limits;

/// Workspace name pattern: 1..=63 chars, leading alphanumeric.
pub const WORKSPACE_NAME_PATTERN: &str = r"^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$";
/// Node (folder/document) name pattern: 1..=128 chars, leading alphanumeric.
pub const NODE_NAME_PATTERN: &str = r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$";

static WORKSPACE_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(WORKSPACE_NAME_PATTERN).expect("workspace name pattern is valid")
});

static NODE_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(NODE_NAME_PATTERN).expect("node name pattern is valid")
});

/// Why a name or path failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// The name is empty.
    Empty,
    /// The name is the reserved `.` or `..` entry.
    Reserved,
    /// The name contains a path separator.
    ContainsSlash,
    /// The name does not match the required pattern (charset / length).
    Pattern,
    /// A folder name must not end with `.md`.
    FolderMdSuffix,
    /// A document name must end with `.md`.
    DocumentMdSuffix,
    /// The path does not start with `/`.
    PathNotAbsolute,
    /// The path exceeds the maximum length.
    PathTooLong,
    /// The path exceeds the maximum depth.
    PathTooDeep,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Empty => "name cannot be empty",
            Self::Reserved => "name cannot be '.' or '..'",
            Self::ContainsSlash => "name cannot contain '/'",
            Self::Pattern => {
                "name must start with a letter or digit and use only letters, digits, '.', '_' or '-'"
            }
            Self::FolderMdSuffix => "folder name cannot end with .md",
            Self::DocumentMdSuffix => "document name must end with .md",
            Self::PathNotAbsolute => "path must start with /",
            Self::PathTooLong => "path is too long",
            Self::PathTooDeep => "path is too deep",
        };
        f.write_str(message)
    }
}

impl std::error::Error for ValidationError {}

/// Validate a workspace name against the shared pattern.
pub fn validate_workspace_name(name: &str) -> Result<(), ValidationError> {
    if name.is_empty() {
        return Err(ValidationError::Empty);
    }
    if !WORKSPACE_NAME_RE.is_match(name) {
        return Err(ValidationError::Pattern);
    }
    Ok(())
}

/// Validate a folder name: shared node pattern, not `.`/`..`, no `.md` suffix.
pub fn validate_folder_name(name: &str) -> Result<(), ValidationError> {
    validate_node_name(name)?;
    if name.ends_with(".md") {
        return Err(ValidationError::FolderMdSuffix);
    }
    Ok(())
}

/// Validate a document name: shared node pattern, not `.`/`..`, `.md` suffix.
pub fn validate_document_name(name: &str) -> Result<(), ValidationError> {
    validate_node_name(name)?;
    if !name.ends_with(".md") {
        return Err(ValidationError::DocumentMdSuffix);
    }
    Ok(())
}

/// Validate a bare node name (folder or document) against the shared pattern.
pub fn validate_node_name(name: &str) -> Result<(), ValidationError> {
    if name.is_empty() {
        return Err(ValidationError::Empty);
    }
    if name == "." || name == ".." {
        return Err(ValidationError::Reserved);
    }
    if name.contains('/') {
        return Err(ValidationError::ContainsSlash);
    }
    if !NODE_NAME_RE.is_match(name) {
        return Err(ValidationError::Pattern);
    }
    Ok(())
}

/// Normalize an absolute path to canonical form (`/a/b`, root = `/`), rejecting
/// `.`/`..` segments and enforcing the path length and depth limits.
pub fn normalize_path(path: &str) -> Result<String, ValidationError> {
    if !path.starts_with('/') {
        return Err(ValidationError::PathNotAbsolute);
    }

    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        if segment == "." || segment == ".." {
            return Err(ValidationError::Reserved);
        }
        segments.push(segment);
    }

    if segments.len() > limits::MAX_PATH_DEPTH {
        return Err(ValidationError::PathTooDeep);
    }

    let normalized = if segments.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", segments.join("/"))
    };

    if normalized.len() > limits::MAX_PATH_LEN {
        return Err(ValidationError::PathTooLong);
    }

    Ok(normalized)
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
    fn workspace_name_accepts_valid_and_rejects_invalid() {
        assert!(validate_workspace_name("notes").is_ok());
        assert!(validate_workspace_name("a.b-c_1").is_ok());
        assert_eq!(validate_workspace_name(""), Err(ValidationError::Empty));
        assert_eq!(
            validate_workspace_name(".hidden"),
            Err(ValidationError::Pattern)
        );
        assert_eq!(
            validate_workspace_name(&"a".repeat(64)),
            Err(ValidationError::Pattern)
        );
    }

    #[test]
    fn folder_name_rejects_md_suffix() {
        assert!(validate_folder_name("notes").is_ok());
        assert_eq!(
            validate_folder_name("notes.md"),
            Err(ValidationError::FolderMdSuffix)
        );
    }

    #[test]
    fn document_name_requires_md_suffix() {
        assert!(validate_document_name("today.md").is_ok());
        assert_eq!(
            validate_document_name("today"),
            Err(ValidationError::DocumentMdSuffix)
        );
    }

    #[test]
    fn node_name_rejects_dotdot_and_slash() {
        assert_eq!(validate_node_name(".."), Err(ValidationError::Reserved));
        assert_eq!(
            validate_node_name("a/b"),
            Err(ValidationError::ContainsSlash)
        );
    }

    #[test]
    fn normalize_path_collapses_and_bounds() {
        assert_eq!(normalize_path("/").unwrap(), "/");
        assert_eq!(normalize_path("/a//b/").unwrap(), "/a/b");
        assert_eq!(
            normalize_path("relative"),
            Err(ValidationError::PathNotAbsolute)
        );
        assert_eq!(normalize_path("/a/../b"), Err(ValidationError::Reserved));
        assert_eq!(
            normalize_path("/a/b/c/d/e/f"),
            Err(ValidationError::PathTooDeep)
        );
    }
}
