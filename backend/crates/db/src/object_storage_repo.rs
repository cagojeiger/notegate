//! Operational cleanup queue for S3-compatible objects.

use notegate_core::Result;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

#[derive(Debug, Clone, FromRow)]
pub struct CleanupCandidate {
    pub id: Uuid,
    pub object_key: String,
    pub state: String,
    pub upload_mode: String,
    pub multipart_upload_id: Option<String>,
    pub retry_count: i32,
}

#[derive(Debug, Clone)]
pub struct ObjectStorageRepo {
    pool: PgPool,
}

impl ObjectStorageRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn claim_cleanup(
        &self,
        stale_after_seconds: i64,
        claim_seconds: i64,
    ) -> Result<Option<CleanupCandidate>> {
        sqlx::query_as::<_, CleanupCandidate>(
            "WITH due AS ( \
                 SELECT id FROM object_storage_objects \
                 WHERE ( \
                     (state = 'uploading' \
                      AND last_activity_at <= now() - ($1 * interval '1 second')) \
                     OR state IN ('expire_pending','delete_pending') \
                 ) \
                 AND (retry_after IS NULL OR retry_after <= now()) \
                 ORDER BY COALESCE(retry_after, last_activity_at), id \
                 FOR UPDATE SKIP LOCKED \
                 LIMIT 1 \
             ) \
             UPDATE object_storage_objects f \
             SET retry_after = now() + ($2 * interval '1 second') \
             FROM due WHERE f.id = due.id \
             RETURNING f.id, f.object_key, f.state, f.upload_mode, \
                 f.multipart_upload_id, f.retry_count",
        )
        .bind(stale_after_seconds)
        .bind(claim_seconds)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn begin_expiry(&self, id: Uuid) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE object_storage_objects \
             SET state = 'expire_pending', last_error_code = NULL \
             WHERE id = $1 AND state = 'uploading' AND retry_after IS NOT NULL",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn mark_expired(&self, id: Uuid) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE object_storage_objects \
             SET state = 'expired', deleted_at = COALESCE(deleted_at, now()), \
                 retry_after = NULL, last_error_code = NULL \
             WHERE id = $1 AND state = 'expire_pending'",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn mark_deleted(&self, id: Uuid) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        // The state transition releases retained bytes via the storage counter
        // trigger. Its receipt must commit with that release, exactly once.
        let completed: Option<(Option<Uuid>, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
            "UPDATE object_storage_objects \
             SET state = 'deleted', deleted_at = COALESCE(deleted_at, now()), \
                 retry_after = NULL, last_error_code = NULL \
             WHERE id = $1 AND state = 'delete_pending' \
             RETURNING usage_space_id, deletion_operation_id, \
                 (SELECT owner_user_id FROM space_storage_usage WHERE space_id = usage_space_id)",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if let Some((space_id, operation_id, owner_user_id)) = completed {
            crate::audit_events::object_deleted(&mut tx, owner_user_id, space_id, id, operation_id)
                .await?;
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(completed.is_some())
    }

    pub async fn mark_cleanup_failed(
        &self,
        id: Uuid,
        error_code: &str,
        retry_seconds: i64,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE object_storage_objects \
             SET retry_count = retry_count + 1, last_error_code = $2, \
                 retry_after = now() + ($3 * interval '1 second') \
             WHERE id = $1 AND state IN ('expire_pending','delete_pending') \
               AND retry_after IS NOT NULL",
        )
        .bind(id)
        .bind(error_code)
        .bind(retry_seconds)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() == 1)
    }
}
