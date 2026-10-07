use notegate_core::Result;
use uuid::Uuid;

use crate::{map_sqlx_error, space_usage};

use super::{SpaceUsageRepo, UsageCounts, configure_transaction, exact_usage};

const STATEMENT_TIMEOUT: &str = "30s";

impl SpaceUsageRepo {
    pub async fn reconcile_space(&self, space_id: Uuid) -> Result<UsageReconcileResult> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        configure_transaction(&mut tx, STATEMENT_TIMEOUT).await?;
        if !space_usage::try_acquire_reconciliation_gate(&mut tx, space_id).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(UsageReconcileResult::Busy);
        }

        let live_space: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM spaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(space_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if live_space.is_none() {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(UsageReconcileResult::Deleted);
        }

        let previous = sqlx::query_as::<_, UsageCounts>(
            "SELECT live_node_count, live_text_bytes, live_file_bytes \
             FROM space_usage WHERE space_id = $1 FOR UPDATE",
        )
        .bind(space_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        // Serialize exact storage repair with S3 completion, whose counter
        // trigger holds this row until its transaction commits.
        sqlx::query("SELECT space_id FROM space_storage_usage WHERE space_id = $1 FOR UPDATE")
            .bind(space_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        sqlx::query(
            "INSERT INTO space_storage_usage(space_id, owner_user_id, text_bytes, file_bytes) \
             SELECT s.id, s.owner_user_id, \
                 COALESCE((SELECT sum(t.byte_len) FROM text_objects t WHERE t.space_id = s.id), 0), \
                 COALESCE((SELECT sum(o.declared_byte_len) FROM object_storage_objects o \
                     WHERE o.usage_space_id = s.id AND o.state IN ('attached', 'delete_pending')), 0) \
             FROM spaces s WHERE s.id = $1 \
             ON CONFLICT (space_id) DO UPDATE SET text_bytes = EXCLUDED.text_bytes, file_bytes = EXCLUDED.file_bytes",
        ).bind(space_id).execute(&mut *tx).await.map_err(map_sqlx_error)?;
        let actual = exact_usage(&mut tx, space_id).await?;
        sqlx::query(
            "INSERT INTO space_usage ( \
                 space_id, live_node_count, live_text_bytes, live_file_bytes, reconciled_at \
             ) VALUES ($1, $2, $3, $4, now()) \
             ON CONFLICT (space_id) DO UPDATE \
             SET live_node_count = EXCLUDED.live_node_count, \
                 live_text_bytes = EXCLUDED.live_text_bytes, \
                 live_file_bytes = EXCLUDED.live_file_bytes, \
                 reconciled_at = EXCLUDED.reconciled_at",
        )
        .bind(space_id)
        .bind(actual.live_node_count)
        .bind(actual.live_text_bytes)
        .bind(actual.live_file_bytes)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)?;

        Ok(UsageReconcileResult::Reconciled { previous, actual })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageReconcileResult {
    Busy,
    Deleted,
    Reconciled {
        previous: Option<UsageCounts>,
        actual: UsageCounts,
    },
}
