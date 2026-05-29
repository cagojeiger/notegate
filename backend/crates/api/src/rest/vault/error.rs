use notegate_db::VaultRepoError;

use crate::error::ApiError;

pub(super) fn map_vault_error(error: VaultRepoError) -> ApiError {
    match error {
        VaultRepoError::NotFound(message) => ApiError::not_found(message),
        VaultRepoError::InvalidInput(message) => ApiError::invalid_field(message),
        VaultRepoError::Conflict(message) => ApiError::conflict(message),
        VaultRepoError::Internal(message) => {
            tracing::error!(event = "vault.error", detail = %message);
            ApiError::internal("internal server error")
        }
    }
}
