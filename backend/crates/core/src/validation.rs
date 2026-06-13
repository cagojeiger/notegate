//! Shared name and path validation.
//!
//! Name rules are intentionally Unicode-friendly: Korean and other scripts are
//! valid. The database `CHECK` constraints in `backend/crates/db/migrations`
//! must stay aligned with these functions.

use crate::limits;

/// Why a name or path failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// The name is empty.
    Empty,
    /// The name is the reserved `.` or `..` entry.
    Reserved,
    /// The name contains a path separator.
    ContainsSlash,
    /// The space name contains `:`, which would make `<space>:/path` targets ambiguous.
    ContainsColon,
    /// The name contains a control character.
    ContainsControl,
    /// The name has leading or trailing whitespace.
    LeadingOrTrailingWhitespace,
    /// The name exceeds the maximum number of Unicode scalar values.
    TooLong {
        /// The maximum allowed character count.
        max: usize,
    },
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
            Self::ContainsColon => "space name cannot contain ':'",
            Self::ContainsControl => "name cannot contain control characters",
            Self::LeadingOrTrailingWhitespace => "name cannot start or end with whitespace",
            Self::TooLong { max } => return write!(f, "name cannot exceed {max} characters"),
            Self::PathNotAbsolute => "path must start with /",
            Self::PathTooLong => "path is too long",
            Self::PathTooDeep => "path is too deep",
        };
        f.write_str(message)
    }
}

impl std::error::Error for ValidationError {}

/// Validate a space name.
///
/// Space names are Unicode-friendly but cannot contain `/` or `:` because MCP
/// compact targets use the `<space>:/path` syntax.
pub fn validate_space_name(name: &str) -> Result<(), ValidationError> {
    validate_common_name(name, limits::SPACE_NAME_MAX_LEN)?;
    if name == "." || name == ".." {
        return Err(ValidationError::Reserved);
    }
    if name.contains('/') {
        return Err(ValidationError::ContainsSlash);
    }
    if name.contains(':') {
        return Err(ValidationError::ContainsColon);
    }
    Ok(())
}

/// Validate a folder name with the shared node-name rule.
pub fn validate_folder_name(name: &str) -> Result<(), ValidationError> {
    validate_node_name(name)
}

/// Validate a text name with the shared node-name rule.
pub fn validate_text_name(name: &str) -> Result<(), ValidationError> {
    validate_node_name(name)
}

/// Validate a file name with the shared node-name rule.
pub fn validate_file_name(name: &str) -> Result<(), ValidationError> {
    validate_node_name(name)
}

/// Validate a bare node name.
///
/// Node names are Unicode-friendly and may contain internal spaces. `/` remains
/// reserved as the path separator.
pub fn validate_node_name(name: &str) -> Result<(), ValidationError> {
    validate_common_name(name, limits::TEXT_NAME_MAX_LEN)?;
    if name == "." || name == ".." {
        return Err(ValidationError::Reserved);
    }
    if name.contains('/') {
        return Err(ValidationError::ContainsSlash);
    }
    Ok(())
}

fn validate_common_name(name: &str, max_chars: usize) -> Result<(), ValidationError> {
    if name.is_empty() {
        return Err(ValidationError::Empty);
    }
    if name.chars().count() > max_chars {
        return Err(ValidationError::TooLong { max: max_chars });
    }
    if name.chars().any(char::is_control) {
        return Err(ValidationError::ContainsControl);
    }
    if name.trim() != name {
        return Err(ValidationError::LeadingOrTrailingWhitespace);
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
    fn space_name_accepts_valid_and_rejects_invalid() {
        assert!(validate_space_name("notes").is_ok());
        assert!(validate_space_name("a.b-c_1").is_ok());
        assert!(validate_space_name("개인 노트").is_ok());
        assert!(validate_space_name(".hidden").is_ok());
        assert_eq!(validate_space_name(""), Err(ValidationError::Empty));
        assert_eq!(validate_space_name("."), Err(ValidationError::Reserved));
        assert_eq!(
            validate_space_name("bad:name"),
            Err(ValidationError::ContainsColon)
        );
        assert_eq!(
            validate_space_name("bad/name"),
            Err(ValidationError::ContainsSlash)
        );
        assert_eq!(
            validate_space_name(" bad"),
            Err(ValidationError::LeadingOrTrailingWhitespace)
        );
        assert_eq!(
            validate_space_name(&"가".repeat(64)),
            Err(ValidationError::TooLong {
                max: limits::SPACE_NAME_MAX_LEN
            })
        );
    }

    #[test]
    fn folder_text_and_file_names_share_node_rules() {
        assert!(validate_folder_name("notes").is_ok());
        assert!(validate_folder_name("회의 자료").is_ok());
        assert!(validate_text_name("상태.json").is_ok());
        assert!(validate_file_name("이미지.png").is_ok());
    }

    #[test]
    fn node_name_rejects_dotdot_and_slash() {
        assert_eq!(validate_node_name(".."), Err(ValidationError::Reserved));
        assert_eq!(
            validate_node_name("a/b"),
            Err(ValidationError::ContainsSlash)
        );
        assert_eq!(
            validate_node_name("name\n"),
            Err(ValidationError::ContainsControl)
        );
        assert_eq!(
            validate_node_name(" trailing "),
            Err(ValidationError::LeadingOrTrailingWhitespace)
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
            normalize_path("/a/b/c/d/e/f/g/h"),
            Err(ValidationError::PathTooDeep)
        );
    }
}
