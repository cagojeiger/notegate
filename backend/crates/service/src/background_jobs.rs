//! Account-scoped background job history pagination.

use notegate_core::limits;
use notegate_db::BackgroundJobRepo;
use notegate_model::{
    BackgroundJobCursor, BackgroundJobDetail, BackgroundJobPage, ListBackgroundJobs,
};
use uuid::Uuid;

use crate::pagination::paginate_keyset;
use crate::{ServiceError, ServiceResult};

pub async fn list_background_job_page(
    jobs: &BackgroundJobRepo,
    owner_account_id: Uuid,
    request: ListBackgroundJobs,
) -> ServiceResult<BackgroundJobPage> {
    let (items, limit, has_more, next_cursor) = paginate_keyset(
        request.limit,
        limits::BACKGROUND_JOBS_DEFAULT_LIMIT,
        limits::BACKGROUND_JOBS_MAX_LIMIT,
        request.cursor.as_deref(),
        |limit, cursor: Option<BackgroundJobCursor>| async move {
            Ok(jobs
                .list_by_owner(owner_account_id, limit, cursor.as_ref())
                .await?)
        },
        |job| BackgroundJobCursor {
            created_at: job.created_at,
            id: job.id,
        },
    )
    .await?;

    Ok(BackgroundJobPage {
        items,
        limit,
        has_more,
        next_cursor,
    })
}

pub async fn get_background_job(
    jobs: &BackgroundJobRepo,
    owner_account_id: Uuid,
    job_id: Uuid,
) -> ServiceResult<BackgroundJobDetail> {
    jobs.get_by_owner(owner_account_id, job_id)
        .await?
        .ok_or_else(|| ServiceError::NotFound("background job not found".to_owned()))
}
