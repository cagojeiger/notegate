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
//!   per-document content over the byte/line cap) are `400` (`InvalidInput`).
//! - Capacity failures against current workspace state (folder fanout, live node
//!   count, live document count, total live document bytes) are `409`
//!   (`Conflict`), carrying an actionable hint.
//!
//! Everything here is pure: no IO, no store access. The service supplies the
//! current counts (children/nodes/documents/bytes) it has already read in the
//! transaction.

use notegate_core::limits;
use notegate_core::validation::{
    self, ValidationError, validate_document_name, validate_folder_name,
};
use notegate_model::NodeKind;

use crate::error::ServiceError;

/// Why a file-tree command failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesValidationError {
    /// A name or path failed format validation (charset, length, depth, `.md`).
    Name(ValidationError),
    /// The parent folder already holds the maximum live direct children.
    FanoutExceeded {
        /// The configured maximum ([`limits::FOLDER_MAX_CHILDREN`]).
        max: usize,
    },
    /// The workspace already holds the maximum live nodes.
    WorkspaceNodesExceeded {
        /// The configured maximum ([`limits::WORKSPACE_MAX_NODES`]).
        max: usize,
    },
    /// The workspace already holds the maximum live documents.
    WorkspaceDocumentsExceeded {
        /// The configured maximum ([`limits::WORKSPACE_MAX_DOCUMENTS`]).
        max: usize,
    },
    /// The document content exceeds the per-document byte cap.
    DocumentBytesExceeded {
        /// The configured maximum ([`limits::DOCUMENT_MAX_BYTES`]).
        max: usize,
    },
    /// The document content exceeds the per-document line cap.
    DocumentLinesExceeded {
        /// The configured maximum ([`limits::DOCUMENT_MAX_LINES`]).
        max: usize,
    },
    /// Storing the document would exceed the workspace's total live byte budget.
    WorkspaceDocumentBytesExceeded {
        /// The configured maximum ([`limits::WORKSPACE_MAX_DOCUMENT_BYTES`]).
        max: usize,
    },
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
            Self::WorkspaceNodesExceeded { max } => ServiceError::Conflict(format!(
                "workspace already has the maximum of {max} live nodes"
            )),
            Self::WorkspaceDocumentsExceeded { max } => ServiceError::Conflict(format!(
                "workspace already has the maximum of {max} live documents"
            )),
            Self::DocumentBytesExceeded { max } => ServiceError::Conflict(format!(
                "document exceeds the maximum of {max} bytes; split the document into smaller notes"
            )),
            Self::DocumentLinesExceeded { max } => ServiceError::Conflict(format!(
                "document exceeds the maximum of {max} lines; split the document into smaller notes"
            )),
            Self::WorkspaceDocumentBytesExceeded { max } => ServiceError::Conflict(format!(
                "workspace live document content would exceed the maximum of {max} bytes; split or move content"
            )),
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

/// Validate a basename for the given kind: shared node format plus the `.md`
/// suffix rule (documents must end with `.md`, folders must not).
pub fn validate_basename(name: &str, kind: NodeKind) -> Result<(), FilesValidationError> {
    match kind {
        NodeKind::Folder => validate_folder_name(name)?,
        NodeKind::Document => validate_document_name(name)?,
    }
    Ok(())
}

/// Normalize and bound an absolute path (rejects `.`/`..`, enforces depth and
/// byte-length limits). Returns the canonical form.
pub fn normalize_path(path: &str) -> Result<String, FilesValidationError> {
    Ok(validation::normalize_path(path)?)
}

/// Reject when the resulting depth of a node would exceed [`limits::MAX_PATH_DEPTH`].
///
/// `depth` is the segment count below the workspace root (root = 0, a direct
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
/// live direct children ([`limits::FOLDER_MAX_CHILDREN`]).
pub fn validate_fanout(live_children: usize) -> Result<(), FilesValidationError> {
    if live_children >= limits::FOLDER_MAX_CHILDREN {
        return Err(FilesValidationError::FanoutExceeded {
            max: limits::FOLDER_MAX_CHILDREN,
        });
    }
    Ok(())
}

/// Reject creating a node when the workspace already holds the maximum live
/// nodes ([`limits::WORKSPACE_MAX_NODES`]).
pub fn validate_workspace_node_count(live_nodes: usize) -> Result<(), FilesValidationError> {
    if live_nodes >= limits::WORKSPACE_MAX_NODES {
        return Err(FilesValidationError::WorkspaceNodesExceeded {
            max: limits::WORKSPACE_MAX_NODES,
        });
    }
    Ok(())
}

/// Reject creating a document when the workspace already holds the maximum live
/// documents ([`limits::WORKSPACE_MAX_DOCUMENTS`]).
pub fn validate_workspace_document_count(
    live_documents: usize,
) -> Result<(), FilesValidationError> {
    if live_documents >= limits::WORKSPACE_MAX_DOCUMENTS {
        return Err(FilesValidationError::WorkspaceDocumentsExceeded {
            max: limits::WORKSPACE_MAX_DOCUMENTS,
        });
    }
    Ok(())
}

/// Reject document content over the per-document byte ([`limits::DOCUMENT_MAX_BYTES`])
/// or line ([`limits::DOCUMENT_MAX_LINES`]) caps. `line_count` is the value
/// computed by [`crate::files::content::compute`].
pub fn validate_document_content(
    byte_len: usize,
    line_count: usize,
) -> Result<(), FilesValidationError> {
    if byte_len > limits::DOCUMENT_MAX_BYTES {
        return Err(FilesValidationError::DocumentBytesExceeded {
            max: limits::DOCUMENT_MAX_BYTES,
        });
    }
    if line_count > limits::DOCUMENT_MAX_LINES {
        return Err(FilesValidationError::DocumentLinesExceeded {
            max: limits::DOCUMENT_MAX_LINES,
        });
    }
    Ok(())
}

/// Reject a write/patch that would push the workspace's total live document
/// bytes over [`limits::WORKSPACE_MAX_DOCUMENT_BYTES`].
///
/// `current_total_bytes` is the workspace's current live document byte sum,
/// `previous_byte_len` is the byte length of the document being replaced (0 for
/// a new document), and `new_byte_len` is the byte length about to be stored.
pub fn validate_workspace_document_bytes(
    current_total_bytes: usize,
    previous_byte_len: usize,
    new_byte_len: usize,
) -> Result<(), FilesValidationError> {
    let projected = current_total_bytes
        .saturating_sub(previous_byte_len)
        .saturating_add(new_byte_len);
    if projected > limits::WORKSPACE_MAX_DOCUMENT_BYTES {
        return Err(FilesValidationError::WorkspaceDocumentBytesExceeded {
            max: limits::WORKSPACE_MAX_DOCUMENT_BYTES,
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
    fn basename_enforces_md_rules_per_kind() {
        assert!(validate_basename("notes", NodeKind::Folder).is_ok());
        assert!(validate_basename("today.md", NodeKind::Document).is_ok());
        assert_eq!(
            validate_basename("notes.md", NodeKind::Folder),
            Err(FilesValidationError::Name(ValidationError::FolderMdSuffix))
        );
        assert_eq!(
            validate_basename("today", NodeKind::Document),
            Err(FilesValidationError::Name(
                ValidationError::DocumentMdSuffix
            ))
        );
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
            validate_basename("", NodeKind::Document),
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
        assert!(validate_fanout(limits::FOLDER_MAX_CHILDREN - 1).is_ok());
        assert_eq!(
            validate_fanout(limits::FOLDER_MAX_CHILDREN),
            Err(FilesValidationError::FanoutExceeded {
                max: limits::FOLDER_MAX_CHILDREN
            })
        );
    }

    #[test]
    fn workspace_node_and_document_count_boundaries() {
        assert!(validate_workspace_node_count(limits::WORKSPACE_MAX_NODES - 1).is_ok());
        assert!(matches!(
            validate_workspace_node_count(limits::WORKSPACE_MAX_NODES),
            Err(FilesValidationError::WorkspaceNodesExceeded { .. })
        ));
        assert!(validate_workspace_document_count(limits::WORKSPACE_MAX_DOCUMENTS - 1).is_ok());
        assert!(matches!(
            validate_workspace_document_count(limits::WORKSPACE_MAX_DOCUMENTS),
            Err(FilesValidationError::WorkspaceDocumentsExceeded { .. })
        ));
    }

    #[test]
    fn document_content_boundaries() {
        assert!(
            validate_document_content(limits::DOCUMENT_MAX_BYTES, limits::DOCUMENT_MAX_LINES)
                .is_ok()
        );
        assert!(matches!(
            validate_document_content(limits::DOCUMENT_MAX_BYTES + 1, 0),
            Err(FilesValidationError::DocumentBytesExceeded { .. })
        ));
        assert!(matches!(
            validate_document_content(0, limits::DOCUMENT_MAX_LINES + 1),
            Err(FilesValidationError::DocumentLinesExceeded { .. })
        ));
    }

    #[test]
    fn workspace_document_bytes_accounts_for_replacement() {
        let max = limits::WORKSPACE_MAX_DOCUMENT_BYTES;
        // Replacing a doc of equal size at the cap stays at the cap (ok).
        assert!(validate_workspace_document_bytes(max, 10, 10).is_ok());
        // Growing past the cap is rejected.
        assert!(matches!(
            validate_workspace_document_bytes(max, 0, 1),
            Err(FilesValidationError::WorkspaceDocumentBytesExceeded { .. })
        ));
        // Shrinking is always fine.
        assert!(validate_workspace_document_bytes(max, 100, 1).is_ok());
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
            FilesValidationError::DocumentBytesExceeded { max: 1 }.into_service_error(),
            ServiceError::Conflict(_)
        ));
    }
}
