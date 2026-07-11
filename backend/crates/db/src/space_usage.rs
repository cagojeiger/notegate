//! Transactional updates for Space usage shadow counters.

use notegate_core::Result;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::map_sqlx_error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UsageDelta {
    pub nodes: i64,
    pub content_bytes: i64,
}

impl UsageDelta {
    pub(crate) const fn new(nodes: i64, content_bytes: i64) -> Self {
        Self {
            nodes,
            content_bytes,
        }
    }
}

/// Apply a derived usage delta without making the shadow counter authoritative.
///
/// A skipped update indicates pre-existing drift, such as a rolling deployment
/// where an older process wrote after the migration backfill. Reconciliation
/// repairs that drift; it must not make an otherwise-valid mutation fail.
pub(crate) async fn apply_shadow_delta(
    tx: &mut PgConnection,
    space_id: Uuid,
    delta: UsageDelta,
) -> Result<()> {
    if delta.nodes == 0 && delta.content_bytes == 0 {
        return Ok(());
    }

    let result = sqlx::query(
        "UPDATE space_usage \
         SET live_node_count = live_node_count + $2, \
             live_content_bytes = live_content_bytes + $3 \
         WHERE space_id = $1 \
           AND live_node_count + $2 >= 1 \
           AND live_content_bytes + $3 >= 0",
    )
    .bind(space_id)
    .bind(delta.nodes)
    .bind(delta.content_bytes)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    if result.rows_affected() == 0 {
        tracing::warn!(
            event = "space_usage.shadow_delta_skipped",
            %space_id,
            nodes = delta.nodes,
            content_bytes = delta.content_bytes,
        );
    }

    Ok(())
}
