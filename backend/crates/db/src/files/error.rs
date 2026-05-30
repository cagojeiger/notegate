#[derive(Debug)]
pub enum FilesRepoError {
    NotFound(String),
    InvalidInput(String),
    Conflict(String),
    Internal(String),
}

pub type FilesResult<T> = Result<T, FilesRepoError>;

pub(super) fn map_sqlx_error(error: sqlx::Error) -> FilesRepoError {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().as_deref() == Some("23505") {
            return FilesRepoError::Conflict("name or path already exists".into());
        }
        if db_error.code().as_deref() == Some("23514") {
            return FilesRepoError::InvalidInput("invalid files data".into());
        }
    }

    FilesRepoError::Internal(format!("files repository query failed: {error}"))
}
