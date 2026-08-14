//! Pure request-local validation for file-tree commands.
//! Format and numeric limits come from `notegate_core`; state-dependent quotas and
//! tree bounds stay in database transactions.

use notegate_core::limits;
use notegate_core::validation::{self, ValidationError, validate_node_name};

use crate::error::ServiceError;

/// Why a file-tree command failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesValidationError {
    /// A name or path failed format validation (charset, length, depth).
    Name(ValidationError),
    /// The object upload exceeds the product file-size cap.
    ObjectFileBytesExceeded { max: usize },
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
    /// Node metadata is not a bounded JSON object.
    MetadataInvalid(String),
}

impl FilesValidationError {
    /// Map this validation failure to the service-layer error the api will turn
    /// into an HTTP status.
    pub fn into_service_error(self) -> ServiceError {
        match self {
            Self::Name(error) => ServiceError::InvalidInput(error.to_string()),
            Self::ObjectFileBytesExceeded { max } => {
                ServiceError::InvalidInput(format!("file exceeds the maximum size of {max} bytes"))
            }
            Self::TextBytesExceeded { max } => ServiceError::InvalidInput(format!(
                "text exceeds the maximum of {max} bytes; split the text into smaller notes"
            )),
            Self::TextLinesExceeded { max } => ServiceError::InvalidInput(format!(
                "text exceeds the maximum of {max} lines; split the text into smaller notes"
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

/// Validate a folder, text, or file basename with the shared node-name rule.
pub fn validate_basename(name: &str) -> Result<(), FilesValidationError> {
    validate_node_name(name)?;
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

pub fn validate_object_file_bytes(byte_len: i64) -> Result<(), FilesValidationError> {
    if byte_len < 0 || byte_len > limits::FILE_MAX_BYTES as i64 {
        return Err(FilesValidationError::ObjectFileBytesExceeded {
            max: limits::FILE_MAX_BYTES,
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
    fn basename_allows_common_file_tree_names() {
        assert!(validate_basename("notes").is_ok());
        assert!(validate_basename("today.md").is_ok());
        assert!(validate_basename("today").is_ok());
        assert!(validate_basename("data.json").is_ok());
    }

    #[test]
    fn basename_rejects_bad_format() {
        assert_eq!(
            validate_basename(".."),
            Err(FilesValidationError::Name(ValidationError::Reserved))
        );
        assert_eq!(
            validate_basename("a/b"),
            Err(FilesValidationError::Name(ValidationError::ContainsSlash))
        );
        assert_eq!(
            validate_basename(""),
            Err(FilesValidationError::Name(ValidationError::Empty))
        );
        // 128-char folder name is the max; Unicode and internal spaces are allowed.
        assert!(validate_basename(&"가".repeat(128)).is_ok());
        assert!(validate_basename("회의 메모.md").is_ok());
        assert_eq!(
            validate_basename(&"가".repeat(129)),
            Err(FilesValidationError::Name(ValidationError::TooLong {
                max: limits::TEXT_NAME_MAX_LEN
            }))
        );
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
    fn object_file_size_boundaries() {
        assert!(validate_object_file_bytes(0).is_ok());
        assert!(validate_object_file_bytes(limits::FILE_MAX_BYTES as i64).is_ok());
        assert!(validate_object_file_bytes(-1).is_err());
        assert!(validate_object_file_bytes(limits::FILE_MAX_BYTES as i64 + 1).is_err());
    }

    #[test]
    fn name_error_maps_to_invalid_input() {
        let err = FilesValidationError::Name(ValidationError::ContainsSlash).into_service_error();
        assert!(matches!(err, ServiceError::InvalidInput(_)));
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
