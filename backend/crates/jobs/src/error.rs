use thiserror::Error;

pub type JobQueueResult<T> = Result<T, JobQueueError>;

#[derive(Debug, Error)]
pub enum JobQueueError {
    #[error("background job database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid background job configuration: {0}")]
    InvalidConfiguration(String),
}
