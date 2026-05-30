use notegate_domain::files::FilesError;

pub(super) fn map_sqlx_error(error: sqlx::Error) -> FilesError {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().as_deref() == Some("23505") {
            return FilesError::Conflict("name or path already exists".into());
        }
        if db_error.code().as_deref() == Some("23514") {
            return FilesError::InvalidInput("invalid files data".into());
        }
    }

    FilesError::Internal(format!("files repository query failed: {error}"))
}
