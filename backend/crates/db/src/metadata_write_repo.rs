//! Batched persistence for best-effort metadata collected by API processes.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{PgPool, map_sqlx_error};
use notegate_core::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTypeObservation {
    pub space_id: Uuid,
    pub node_id: Uuid,
    pub media_type: String,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct MetadataWriteRepo {
    pool: PgPool,
}

impl MetadataWriteRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn update_api_key_last_used(&self, ids: &[Uuid]) -> Result<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let result = sqlx::query(
            "UPDATE api_keys \
             SET last_used_at = now() \
             WHERE id = ANY($1) \
               AND (last_used_at IS NULL OR last_used_at < now() - interval '1 hour')",
        )
        .bind(ids)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected())
    }

    pub async fn update_browser_session_last_used(&self, ids: &[Uuid]) -> Result<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let result = sqlx::query(
            "UPDATE browser_sessions \
             SET last_used_at = now(), updated_at = now() \
             WHERE id = ANY($1) \
               AND (last_used_at IS NULL OR last_used_at < now() - interval '1 hour')",
        )
        .bind(ids)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected())
    }

    pub async fn set_detected_media_types(
        &self,
        observations: &[MediaTypeObservation],
    ) -> Result<u64> {
        if observations.is_empty() {
            return Ok(0);
        }

        let space_ids = observations
            .iter()
            .map(|observation| observation.space_id)
            .collect::<Vec<_>>();
        let node_ids = observations
            .iter()
            .map(|observation| observation.node_id)
            .collect::<Vec<_>>();
        let media_types = observations
            .iter()
            .map(|observation| observation.media_type.as_str())
            .collect::<Vec<_>>();
        let observed_at = observations
            .iter()
            .map(|observation| observation.observed_at)
            .collect::<Vec<_>>();

        let result = sqlx::query(
            "WITH detected AS ( \
                 SELECT DISTINCT ON (space_id, node_id) space_id, node_id, media_type \
                 FROM UNNEST($1::uuid[], $2::uuid[], $3::text[], $4::timestamptz[]) \
                      AS observed(space_id, node_id, media_type, observed_at) \
                 ORDER BY space_id, node_id, observed_at, media_type \
             ) \
             UPDATE file_objects AS file \
             SET detected_media_type = detected.media_type \
             FROM detected \
             WHERE file.space_id = detected.space_id \
               AND file.node_id = detected.node_id \
               AND (file.detected_media_type IS NULL \
                    OR (file.detected_media_type = 'application/zip' \
                        AND detected.media_type = 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'))",
        )
        .bind(space_ids)
        .bind(node_ids)
        .bind(media_types)
        .bind(observed_at)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected())
    }
}
