use notegate_domain::files::FilesError;

use crate::error::ApiError;

pub(super) fn map_files_error(error: FilesError) -> ApiError {
    match error {
        FilesError::NotFound(message) => ApiError::not_found(message),
        FilesError::InvalidInput(message) => ApiError::invalid_field(message),
        FilesError::Conflict(message) => ApiError::conflict(message),
        FilesError::Internal(message) => {
            tracing::error!(event = "files.error", detail = %message);
            ApiError::internal("internal server error")
        }
    }
}
