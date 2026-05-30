use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesError {
    NotFound(String),
    InvalidInput(String),
    Conflict(String),
    Internal(String),
}

pub type FilesResult<T> = Result<T, FilesError>;

impl fmt::Display for FilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message)
            | Self::InvalidInput(message)
            | Self::Conflict(message)
            | Self::Internal(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for FilesError {}
