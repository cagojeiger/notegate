use notegate_db::FilesRepoError;

use crate::error::ApiError;

pub(super) fn map_files_error(error: FilesRepoError) -> ApiError {
    match error {
        FilesRepoError::NotFound(message) => ApiError::not_found(message),
        FilesRepoError::InvalidInput(message) => ApiError::invalid_field(message),
        FilesRepoError::Conflict(message) => ApiError::conflict(message),
        FilesRepoError::Internal(message) => {
            tracing::error!(event = "files.error", detail = %message);
            ApiError::internal("internal server error")
        }
    }
}
