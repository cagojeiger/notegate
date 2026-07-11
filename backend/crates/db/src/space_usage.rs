//! Transactional updates for Space usage shadow counters.

use notegate_core::Result;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::map_sqlx_error;

const SPACE_GATE_NAMESPACE: u64 = 0x4e47_5350_4143_4501;
const FULL_RECONCILIATION_GATE_KEY: i64 = 0x4e47_5553_4147_4502;
const MUTATION_RETRY_AFTER_SECONDS: u64 = 5;

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

/// Acquire the shared side of the Space reconciliation gate without waiting.
pub(crate) async fn acquire_mutation_gate(tx: &mut PgConnection, space_id: Uuid) -> Result<()> {
    if try_full_reconciliation_gate(tx, true).await? && try_space_gate(tx, space_id, true).await? {
        return Ok(());
    }

    Err(notegate_core::Error::usage_recalculation_in_progress(
        MUTATION_RETRY_AFTER_SECONDS,
    ))
}

/// Try to acquire exclusive reconciliation access for one Space.
pub(crate) async fn try_acquire_reconciliation_gate(
    tx: &mut PgConnection,
    space_id: Uuid,
) -> Result<bool> {
    if !try_full_reconciliation_gate(tx, true).await? {
        return Ok(false);
    }
    try_space_gate(tx, space_id, false).await
}

/// Try to block every file-tree mutation for an atomic full recalculation.
pub(crate) async fn try_acquire_full_reconciliation_gate(tx: &mut PgConnection) -> Result<bool> {
    try_full_reconciliation_gate(tx, false).await
}

async fn try_full_reconciliation_gate(tx: &mut PgConnection, shared: bool) -> Result<bool> {
    let query = if shared {
        "SELECT pg_try_advisory_xact_lock_shared($1)"
    } else {
        "SELECT pg_try_advisory_xact_lock($1)"
    };
    sqlx::query_scalar(query)
        .bind(FULL_RECONCILIATION_GATE_KEY)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)
}

async fn try_space_gate(tx: &mut PgConnection, space_id: Uuid, shared: bool) -> Result<bool> {
    let query = if shared {
        "SELECT pg_try_advisory_xact_lock_shared($1)"
    } else {
        "SELECT pg_try_advisory_xact_lock($1)"
    };
    sqlx::query_scalar(query)
        .bind(space_gate_key(space_id))
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)
}

fn space_gate_key(space_id: Uuid) -> i64 {
    let value = space_id.as_u128();
    let folded = (value as u64) ^ ((value >> 64) as u64) ^ SPACE_GATE_NAMESPACE;
    i64::from_ne_bytes(folded.to_ne_bytes())
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
