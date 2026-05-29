#[derive(Debug)]
pub enum VaultRepoError {
    NotFound(String),
    InvalidInput(String),
    Conflict(String),
    Internal(String),
}

pub type VaultResult<T> = Result<T, VaultRepoError>;

pub(super) fn map_sqlx_error(error: sqlx::Error) -> VaultRepoError {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().as_deref() == Some("23505") {
            return VaultRepoError::Conflict("name or path already exists".into());
        }
        if db_error.code().as_deref() == Some("23514") {
            return VaultRepoError::InvalidInput("invalid vault data".into());
        }
    }

    VaultRepoError::Internal(format!("vault repository query failed: {error}"))
}
