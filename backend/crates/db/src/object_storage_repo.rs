//! Operational cleanup queue for S3-compatible objects.

use notegate_core::Result;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

#[derive(Debug, Clone, FromRow)]
pub struct CleanupCandidate {
    pub id: Uuid,
    pub object_key: String,
    pub state: String,
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
             RETURNING f.id, f.object_key, f.state, f.retry_count",
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
        let result = sqlx::query(
            "UPDATE object_storage_objects \
             SET state = 'deleted', deleted_at = COALESCE(deleted_at, now()), \
                 retry_after = NULL, last_error_code = NULL \
             WHERE id = $1 AND state IN ('delete_pending','deleted')",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() == 1)
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

    pub async fn purge_terminal_history(&self, retention_days: i32, limit: i64) -> Result<u64> {
        let result = sqlx::query(
            "WITH due AS ( \
                 SELECT id FROM object_storage_objects \
                 WHERE state IN ('expired','deleted') \
                   AND COALESCE(deleted_at, last_activity_at) \
                       <= now() - make_interval(days => $1) \
                 ORDER BY COALESCE(deleted_at, last_activity_at), id \
                 LIMIT $2 \
             ) \
             DELETE FROM object_storage_objects f USING due WHERE f.id = due.id",
        )
        .bind(retention_days)
        .bind(limit)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected())
    }
}

pub(crate) async fn queue_space_object_deletions(
    tx: &mut PgConnection,
    space_id: Uuid,
) -> Result<()> {
    sqlx::query(
        "UPDATE object_storage_objects \
         SET state = 'delete_pending', \
             delete_requested_at = COALESCE(delete_requested_at, now()), \
             retry_after = NULL, last_error_code = NULL \
         WHERE state = 'attached' AND space_id = $1",
    )
    .bind(space_id)
    .execute(tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

pub(crate) async fn queue_subtree_object_deletions(
    tx: &mut PgConnection,
    space_id: Uuid,
    root_node_id: Uuid,
) -> Result<()> {
    sqlx::query(
        "WITH RECURSIVE subtree AS ( \
             SELECT id FROM nodes \
             WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
             UNION ALL \
             SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
             WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         UPDATE object_storage_objects \
         SET state = 'delete_pending', \
             delete_requested_at = COALESCE(delete_requested_at, now()), \
             retry_after = NULL, last_error_code = NULL \
         WHERE state = 'attached' AND node_id IN (SELECT id FROM subtree)",
    )
    .bind(space_id)
    .bind(root_node_id)
    .execute(tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}
