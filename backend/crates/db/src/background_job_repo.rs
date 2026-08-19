//! Account-scoped history for jobs submitted to the generic background queue.

use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_model::{
    BackgroundJob, BackgroundJobAttempt, BackgroundJobCursor, BackgroundJobDetail,
};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

#[derive(Debug, Clone)]
pub struct BackgroundJobRepo {
    pool: PgPool,
}

impl BackgroundJobRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list_by_owner(
        &self,
        owner_account_id: Uuid,
        limit: i64,
        cursor: Option<&BackgroundJobCursor>,
    ) -> Result<Vec<BackgroundJob>> {
        let cursor_created_at = cursor.map(|cursor| cursor.created_at);
        let cursor_id = cursor.map(|cursor| cursor.id);
        let rows = sqlx::query_as::<_, BackgroundJobRow>(
            "SELECT job.job_id, job.job_kind, job.status, \
                    COALESCE(job.context_kind, CASE WHEN link_space.id IS NOT NULL \
                        THEN 'space' END) AS context_kind, \
                    COALESCE(job.context_id, link_space.id) AS context_id, \
                    COALESCE(job.context_label, link_space.name) AS context_label, \
                    job.attempt_count, job.failure_count, job.max_attempts, \
                    job.last_error_code, job.created_at, job.updated_at, job.completed_at \
             FROM background_jobs job \
             LEFT JOIN spaces link_space \
               ON job.job_kind = 'link_graph_project_nodes' \
              AND link_space.owner_user_id = $1 \
              AND link_space.id::text = job.payload ->> 'space_id' \
             WHERE ((job.history_visibility = 'visible' \
                     AND job.history_owner_account_id = $1) \
                    OR link_space.id IS NOT NULL) \
               AND ($2::timestamptz IS NULL OR (job.created_at, job.job_id) < ($2, $3)) \
             ORDER BY job.created_at DESC, job.job_id DESC \
             LIMIT $4",
        )
        .bind(owner_account_id)
        .bind(cursor_created_at)
        .bind(cursor_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(rows.into_iter().map(BackgroundJob::from).collect())
    }

    pub async fn get_by_owner(
        &self,
        owner_account_id: Uuid,
        job_id: Uuid,
    ) -> Result<Option<BackgroundJobDetail>> {
        let row = sqlx::query_as::<_, BackgroundJobRow>(
            "SELECT job.job_id, job.job_kind, job.status, \
                    COALESCE(job.context_kind, CASE WHEN link_space.id IS NOT NULL \
                        THEN 'space' END) AS context_kind, \
                    COALESCE(job.context_id, link_space.id) AS context_id, \
                    COALESCE(job.context_label, link_space.name) AS context_label, \
                    job.attempt_count, job.failure_count, job.max_attempts, \
                    job.last_error_code, job.created_at, job.updated_at, job.completed_at \
             FROM background_jobs job \
             LEFT JOIN spaces link_space \
               ON job.job_kind = 'link_graph_project_nodes' \
              AND link_space.owner_user_id = $2 \
              AND link_space.id::text = job.payload ->> 'space_id' \
             WHERE job.job_id = $1 \
               AND ((job.history_visibility = 'visible' \
                     AND job.history_owner_account_id = $2) \
                    OR link_space.id IS NOT NULL)",
        )
        .bind(job_id)
        .bind(owner_account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        let Some(row) = row else {
            return Ok(None);
        };

        let attempts = sqlx::query_as::<_, BackgroundJobAttemptRow>(
            "SELECT attempt_number, started_at, finished_at, outcome, error_code \
             FROM background_job_attempts \
             WHERE job_id = $1 \
             ORDER BY attempt_number DESC",
        )
        .bind(job_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(Some(BackgroundJobDetail {
            job: row.into(),
            attempts: attempts
                .into_iter()
                .map(BackgroundJobAttempt::from)
                .collect(),
        }))
    }
}

#[derive(Debug, FromRow)]
struct BackgroundJobRow {
    job_id: Uuid,
    job_kind: String,
    status: String,
    context_kind: Option<String>,
    context_id: Option<Uuid>,
    context_label: Option<String>,
    attempt_count: i32,
    failure_count: i32,
    max_attempts: i32,
    last_error_code: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

impl From<BackgroundJobRow> for BackgroundJob {
    fn from(row: BackgroundJobRow) -> Self {
        Self {
            id: row.job_id,
            kind: row.job_kind,
            status: row.status,
            context_kind: row.context_kind,
            context_id: row.context_id,
            context_label: row.context_label,
            attempt_count: row.attempt_count,
            failure_count: row.failure_count,
            max_attempts: row.max_attempts,
            last_error_code: row.last_error_code,
            created_at: row.created_at,
            updated_at: row.updated_at,
            completed_at: row.completed_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct BackgroundJobAttemptRow {
    attempt_number: i32,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    outcome: Option<String>,
    error_code: Option<String>,
}

impl From<BackgroundJobAttemptRow> for BackgroundJobAttempt {
    fn from(row: BackgroundJobAttemptRow) -> Self {
        Self {
            attempt_number: row.attempt_number,
            started_at: row.started_at,
            finished_at: row.finished_at,
            outcome: row.outcome,
            error_code: row.error_code,
        }
    }
}
