//! Pure validation for file-tree commands.
//!
//! These functions are the single in-process gate for the name, path, depth,
//! fanout, and size limits in `docs/spec/{files-commands,performance-limits,db}.md`.
//! They build on [`notegate_core::validation`] (name/path format, shared with the
//! DB `CHECK` constraints) and [`notegate_core::limits`] (the numeric caps), and
//! return a typed [`FilesValidationError`] so the service can map each failure to
//! the correct HTTP status.
//!
//! Status mapping (see [`FilesValidationError::into_service_error`]):
//! - Format-of-input failures (bad name, non-absolute/too-long/too-deep path,
//!   per-text content over the byte/line cap) are `400` (`InvalidInput`).
//! - Capacity failures against current space state (folder fanout, live node
//!   count, total live content bytes) are `409`
//!   (`Conflict`), carrying an actionable hint.
//!
//! Everything here is pure: no IO, no store access. The service supplies the
//! current counts (children/nodes/texts/bytes) it read for pre-checks; the
//! DB repository re-checks capacity inside each mutating transaction.

use notegate_core::limits::{self, Limits};
use notegate_core::validation::{self, ValidationError, validate_folder_name, validate_text_name};
use notegate_model::NodeKind;

use crate::error::ServiceError;

/// Why a file-tree command failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesValidationError {
    /// A name or path failed format validation (charset, length, depth).
    Name(ValidationError),
    /// The parent folder already holds the maximum live direct children.
    FanoutExceeded {
        /// The configured maximum.
        max: usize,
    },
    /// The space already holds the maximum live nodes.
    SpaceNodesExceeded {
        /// The configured maximum.
        max: usize,
    },
    /// The uploaded file exceeds the inline PostgreSQL byte cap.
    FileBytesExceeded {
        /// The configured maximum ([`limits::FILE_INLINE_PG_MAX_BYTES`]).
        max: usize,
    },
    /// The text content exceeds the per-text byte cap.
    TextBytesExceeded {
        /// The configured maximum ([`limits::TEXT_MAX_BYTES`]).
        max: usize,
    },
    /// The text content exceeds the per-text line cap.
    TextLinesExceeded {
        /// The configured maximum ([`limits::TEXT_MAX_LINES`]).
        max: usize,
    },
    /// Storing content would exceed the space's total live byte budget.
    SpaceContentBytesExceeded {
        /// The configured maximum.
        max: usize,
    },
    /// Node metadata is not a bounded JSON object.
    MetadataInvalid(String),
}

impl FilesValidationError {
    /// Map this validation failure to the service-layer error the api will turn
    /// into an HTTP status. Format failures become `400`; capacity and size
    /// failures become `409` with a hint.
    pub fn into_service_error(self) -> ServiceError {
        match self {
            Self::Name(error) => ServiceError::InvalidInput(error.to_string()),
            Self::FanoutExceeded { max } => ServiceError::Conflict(format!(
                "folder already has the maximum of {max} live children; split into subfolders"
            )),
            Self::SpaceNodesExceeded { max } => {
                ServiceError::Conflict(format!("space already has the maximum of {max} live nodes"))
            }
            Self::FileBytesExceeded { max } => ServiceError::InvalidInput(format!(
                "file exceeds the maximum inline size of {max} bytes; object storage is not enabled"
            )),
            Self::TextBytesExceeded { max } => ServiceError::InvalidInput(format!(
                "text exceeds the maximum of {max} bytes; split the text into smaller notes"
            )),
            Self::TextLinesExceeded { max } => ServiceError::InvalidInput(format!(
                "text exceeds the maximum of {max} lines; split the text into smaller notes"
            )),
            Self::SpaceContentBytesExceeded { max } => ServiceError::Conflict(format!(
                "space content would exceed the maximum of {max} bytes; delete, move, or split content"
            )),
            Self::MetadataInvalid(message) => ServiceError::InvalidInput(message),
        }
    }
}

impl From<ValidationError> for FilesValidationError {
    fn from(error: ValidationError) -> Self {
        Self::Name(error)
    }
}

impl From<FilesValidationError> for ServiceError {
    fn from(error: FilesValidationError) -> Self {
        error.into_service_error()
    }
}

/// Validate a basename for the given kind using the shared node-name rule for
/// folders, text nodes, and file nodes.
pub fn validate_basename(name: &str, kind: NodeKind) -> Result<(), FilesValidationError> {
    match kind {
        NodeKind::Folder => validate_folder_name(name)?,
        NodeKind::Text => validate_text_name(name)?,
        NodeKind::File => notegate_core::validation::validate_file_name(name)?,
    }
    Ok(())
}

/// Normalize and bound an absolute path (rejects `.`/`..`, enforces depth and
/// byte-length limits). Returns the canonical form.
pub fn normalize_path(path: &str) -> Result<String, FilesValidationError> {
    Ok(validation::normalize_path(path)?)
}

/// Reject node metadata that cannot be safely stored or searched.
pub fn validate_metadata(metadata: &serde_json::Value) -> Result<(), FilesValidationError> {
    if !metadata.is_object() {
        return Err(FilesValidationError::MetadataInvalid(
            "metadata must be a JSON object".to_owned(),
        ));
    }

    let bytes = serde_json::to_vec(metadata)
        .map_err(|error| FilesValidationError::MetadataInvalid(error.to_string()))?
        .len();
    if bytes > limits::NODE_METADATA_MAX_BYTES {
        return Err(FilesValidationError::MetadataInvalid(format!(
            "metadata exceeds the maximum of {} bytes",
            limits::NODE_METADATA_MAX_BYTES
        )));
    }

    validate_metadata_value(metadata, 1)
}

fn validate_metadata_value(
    value: &serde_json::Value,
    depth: usize,
) -> Result<(), FilesValidationError> {
    if depth > limits::NODE_METADATA_MAX_DEPTH {
        return Err(FilesValidationError::MetadataInvalid(format!(
            "metadata exceeds the maximum depth of {}",
            limits::NODE_METADATA_MAX_DEPTH
        )));
    }

    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if key.chars().count() > limits::NODE_METADATA_KEY_MAX_CHARS {
                    return Err(FilesValidationError::MetadataInvalid(format!(
                        "metadata key exceeds the maximum of {} characters",
                        limits::NODE_METADATA_KEY_MAX_CHARS
                    )));
                }
                validate_metadata_value(value, depth + 1)?;
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                validate_metadata_value(item, depth + 1)?;
            }
        }
        serde_json::Value::String(value) => {
            if value.chars().count() > limits::NODE_METADATA_STRING_MAX_CHARS {
                return Err(FilesValidationError::MetadataInvalid(format!(
                    "metadata string value exceeds the maximum of {} characters",
                    limits::NODE_METADATA_STRING_MAX_CHARS
                )));
            }
        }
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) | serde_json::Value::Null => {}
    }
    Ok(())
}

/// Reject when the resulting depth of a node would exceed [`limits::MAX_PATH_DEPTH`].
///
/// `depth` is the segment count below the space root (root = 0, a direct
/// child of root = 1). The created/moved node's own depth is what is checked.
pub fn validate_depth(depth: usize) -> Result<(), FilesValidationError> {
    if depth > limits::MAX_PATH_DEPTH {
        return Err(FilesValidationError::Name(ValidationError::PathTooDeep));
    }
    Ok(())
}

/// Reject when the derived path length would exceed [`limits::MAX_PATH_LEN`] bytes.
pub fn validate_path_len(path: &str) -> Result<(), FilesValidationError> {
    if path.len() > limits::MAX_PATH_LEN {
        return Err(FilesValidationError::Name(ValidationError::PathTooLong));
    }
    Ok(())
}

/// Reject creating/moving a child into a parent that already holds the maximum
/// live direct children.
pub fn validate_fanout(live_children: usize, limits: Limits) -> Result<(), FilesValidationError> {
    if live_children >= limits.folder_max_children {
        return Err(FilesValidationError::FanoutExceeded {
            max: limits.folder_max_children,
        });
    }
    Ok(())
}

/// Reject creating a node when the space already holds the maximum live
/// nodes.
pub fn validate_space_node_count(
    live_nodes: usize,
    limits: Limits,
) -> Result<(), FilesValidationError> {
    if live_nodes >= limits.space_max_nodes {
        return Err(FilesValidationError::SpaceNodesExceeded {
            max: limits.space_max_nodes,
        });
    }
    Ok(())
}

/// Reject files larger than the current inline PostgreSQL cap.
pub fn validate_file_bytes(byte_len: usize) -> Result<(), FilesValidationError> {
    if byte_len > limits::FILE_INLINE_PG_MAX_BYTES {
        return Err(FilesValidationError::FileBytesExceeded {
            max: limits::FILE_INLINE_PG_MAX_BYTES,
        });
    }
    Ok(())
}

pub fn validate_text_content(
    byte_len: usize,
    line_count: usize,
) -> Result<(), FilesValidationError> {
    if byte_len > limits::TEXT_MAX_BYTES {
        return Err(FilesValidationError::TextBytesExceeded {
            max: limits::TEXT_MAX_BYTES,
        });
    }
    if line_count > limits::TEXT_MAX_LINES {
        return Err(FilesValidationError::TextLinesExceeded {
            max: limits::TEXT_MAX_LINES,
        });
    }
    Ok(())
}

/// Reject a mutation that would push the space's total live content bytes
/// over the configured space byte budget.
///
/// `current_total_bytes` is the space's current live text+file byte sum,
/// `previous_byte_len` is the byte length being replaced (0 for new content),
/// and `new_byte_len` is the byte length about to be stored.
pub fn validate_space_content_bytes(
    current_total_bytes: usize,
    previous_byte_len: usize,
    new_byte_len: usize,
    limits: Limits,
) -> Result<(), FilesValidationError> {
    let projected = current_total_bytes
        .saturating_sub(previous_byte_len)
        .saturating_add(new_byte_len);
    if projected > limits.space_max_content_bytes {
        return Err(FilesValidationError::SpaceContentBytesExceeded {
            max: limits.space_max_content_bytes,
        });
    }
    Ok(())
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
    fn basename_allows_common_file_tree_names_per_kind() {
        assert!(validate_basename("notes", NodeKind::Folder).is_ok());
        assert!(validate_basename("today.md", NodeKind::Text).is_ok());
        assert!(validate_basename("today", NodeKind::Text).is_ok());
        assert!(validate_basename("data.json", NodeKind::File).is_ok());
    }

    #[test]
    fn basename_rejects_bad_format() {
        assert_eq!(
            validate_basename("..", NodeKind::Folder),
            Err(FilesValidationError::Name(ValidationError::Reserved))
        );
        assert_eq!(
            validate_basename("a/b", NodeKind::Folder),
            Err(FilesValidationError::Name(ValidationError::ContainsSlash))
        );
        assert_eq!(
            validate_basename("", NodeKind::Text),
            Err(FilesValidationError::Name(ValidationError::Empty))
        );
        // 128-char folder name is the max; 129 is rejected by the pattern.
        assert!(validate_basename(&"a".repeat(128), NodeKind::Folder).is_ok());
        assert_eq!(
            validate_basename(&"a".repeat(129), NodeKind::Folder),
            Err(FilesValidationError::Name(ValidationError::Pattern))
        );
    }

    #[test]
    fn depth_boundary() {
        assert!(validate_depth(0).is_ok());
        assert!(validate_depth(limits::MAX_PATH_DEPTH).is_ok());
        assert_eq!(
            validate_depth(limits::MAX_PATH_DEPTH + 1),
            Err(FilesValidationError::Name(ValidationError::PathTooDeep))
        );
    }

    #[test]
    fn path_len_boundary() {
        assert!(validate_path_len(&"a".repeat(limits::MAX_PATH_LEN)).is_ok());
        assert_eq!(
            validate_path_len(&"a".repeat(limits::MAX_PATH_LEN + 1)),
            Err(FilesValidationError::Name(ValidationError::PathTooLong))
        );
    }

    #[test]
    fn fanout_boundary() {
        let caps = Limits::default();
        assert!(validate_fanout(limits::FOLDER_MAX_CHILDREN - 1, caps).is_ok());
        assert_eq!(
            validate_fanout(limits::FOLDER_MAX_CHILDREN, caps),
            Err(FilesValidationError::FanoutExceeded {
                max: limits::FOLDER_MAX_CHILDREN
            })
        );
    }

    #[test]
    fn space_node_count_boundaries() {
        let caps = Limits::default();
        assert!(validate_space_node_count(limits::SPACE_MAX_NODES - 1, caps).is_ok());
        assert!(matches!(
            validate_space_node_count(limits::SPACE_MAX_NODES, caps),
            Err(FilesValidationError::SpaceNodesExceeded { .. })
        ));
    }

    #[test]
    fn text_content_boundaries() {
        assert!(validate_text_content(limits::TEXT_MAX_BYTES, limits::TEXT_MAX_LINES).is_ok());
        assert!(matches!(
            validate_text_content(limits::TEXT_MAX_BYTES + 1, 0),
            Err(FilesValidationError::TextBytesExceeded { .. })
        ));
        assert!(matches!(
            validate_text_content(0, limits::TEXT_MAX_LINES + 1),
            Err(FilesValidationError::TextLinesExceeded { .. })
        ));
    }

    #[test]
    fn space_content_bytes_accounts_for_replacement() {
        let max = limits::SPACE_MAX_CONTENT_BYTES;
        let caps = Limits::default();
        // Replacing a doc of equal size at the cap stays at the cap (ok).
        assert!(validate_space_content_bytes(max, 10, 10, caps).is_ok());
        // Growing past the cap is rejected.
        assert!(matches!(
            validate_space_content_bytes(max, 0, 1, caps),
            Err(FilesValidationError::SpaceContentBytesExceeded { .. })
        ));
        // Shrinking is always fine.
        assert!(validate_space_content_bytes(max, 100, 1, caps).is_ok());
    }

    #[test]
    fn name_error_maps_to_invalid_input() {
        let err = FilesValidationError::Name(ValidationError::Pattern).into_service_error();
        assert!(matches!(err, ServiceError::InvalidInput(_)));
    }

    #[test]
    fn capacity_errors_map_to_conflict() {
        assert!(matches!(
            FilesValidationError::FanoutExceeded { max: 200 }.into_service_error(),
            ServiceError::Conflict(_)
        ));
        assert!(matches!(
            FilesValidationError::SpaceContentBytesExceeded { max: 1 }.into_service_error(),
            ServiceError::Conflict(_)
        ));
    }

    #[test]
    fn per_text_size_errors_map_to_invalid_input() {
        assert!(matches!(
            FilesValidationError::TextBytesExceeded { max: 1 }.into_service_error(),
            ServiceError::InvalidInput(_)
        ));
        assert!(matches!(
            FilesValidationError::TextLinesExceeded { max: 1 }.into_service_error(),
            ServiceError::InvalidInput(_)
        ));
    }
}
