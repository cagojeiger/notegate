//! Transactional quota enforcement and updates for Space usage counters.
//!
//! Lock order for every path that touches a Space's counters: per-Space
//! gate → space owner (accounts row) → spaces row → space_usage row.
//! [`MutationGate`] witnesses the gate step, so counter mutations cannot
//! compile without it.

use notegate_core::limits::Limits;
use notegate_core::{Error, Result};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::map_sqlx_error;

const SPACE_GATE_NAMESPACE: u64 = 0x4e47_5350_4143_4501;
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

/// Proof that the reconciliation gate is held for one Space's mutation.
/// Only [`acquire_mutation_gate`] constructs it, so counter mutations cannot
/// run without the gate and always target the gated Space.
#[derive(Debug)]
pub(crate) struct MutationGate {
    space_id: Uuid,
}

/// Acquire the shared side of the Space reconciliation gate without waiting.
pub(crate) async fn acquire_mutation_gate(
    tx: &mut PgConnection,
    space_id: Uuid,
) -> Result<MutationGate> {
    if !try_space_gate(tx, space_id, true).await? {
        return Err(recalculation_in_progress());
    }
    Ok(MutationGate { space_id })
}

fn recalculation_in_progress() -> Error {
    Error::usage_recalculation_in_progress(MUTATION_RETRY_AFTER_SECONDS)
}

/// Try to acquire exclusive reconciliation access for one Space.
pub(crate) async fn try_acquire_reconciliation_gate(
    tx: &mut PgConnection,
    space_id: Uuid,
) -> Result<bool> {
    try_space_gate(tx, space_id, false).await
}

async fn try_space_gate(tx: &mut PgConnection, space_id: Uuid, shared: bool) -> Result<bool> {
    try_schema_advisory_lock(tx, space_gate_seed(space_id), shared).await
}

pub(crate) async fn try_schema_advisory_lock(
    tx: &mut PgConnection,
    seed: i64,
    shared: bool,
) -> Result<bool> {
    let query = if shared {
        "SELECT pg_try_advisory_xact_lock_shared(\
             hashtextextended(current_schema(), $1)\
         )"
    } else {
        "SELECT pg_try_advisory_xact_lock(\
             hashtextextended(current_schema(), $1)\
         )"
    };
    sqlx::query_scalar(query)
        .bind(seed)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)
}

fn space_gate_seed(space_id: Uuid) -> i64 {
    let value = space_id.as_u128();
    let folded = (value as u64) ^ ((value >> 64) as u64) ^ SPACE_GATE_NAMESPACE;
    i64::from_ne_bytes(folded.to_ne_bytes())
}

/// Validate a quota-affecting delta and reserve it before source rows change.
pub(crate) async fn apply_quota_delta(
    tx: &mut PgConnection,
    gate: &MutationGate,
    delta: UsageDelta,
    limits: Limits,
) -> Result<()> {
    apply_delta(tx, gate.space_id, delta, Some(limits)).await
}

/// Release usage for a soft delete without blocking cleanup of over-limit data.
pub(crate) async fn release_usage(
    tx: &mut PgConnection,
    gate: &MutationGate,
    delta: UsageDelta,
) -> Result<()> {
    if delta.nodes > 0 || delta.content_bytes > 0 {
        return Err(Error::internal("usage release delta must not be positive"));
    }
    apply_delta(tx, gate.space_id, delta, None).await
}

async fn apply_delta(
    tx: &mut PgConnection,
    space_id: Uuid,
    delta: UsageDelta,
    limits: Option<Limits>,
) -> Result<()> {
    let current: Option<(i64, i64)> = sqlx::query_as(
        "SELECT live_node_count, live_content_bytes \
         FROM space_usage WHERE space_id = $1 FOR UPDATE",
    )
    .bind(space_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let (current_nodes, current_content_bytes) =
        current.ok_or_else(|| Error::internal("live space is missing its usage counter"))?;

    let projected_nodes = current_nodes
        .checked_add(delta.nodes)
        .ok_or_else(|| Error::internal("space usage node counter overflow"))?;
    let projected_content_bytes = current_content_bytes
        .checked_add(delta.content_bytes)
        .ok_or_else(|| Error::internal("space usage content counter overflow"))?;
    if projected_nodes < 1 || projected_content_bytes < 0 {
        return Err(Error::internal("space usage counter underflow"));
    }

    if let Some(limits) = limits {
        let max_nodes = i64::try_from(limits.space_max_nodes)
            .map_err(|_error| Error::internal("space node limit exceeds bigint"))?;
        let max_content_bytes = i64::try_from(limits.space_max_content_bytes)
            .map_err(|_error| Error::internal("space content limit exceeds bigint"))?;
        if delta.nodes > 0 && projected_nodes > max_nodes {
            return Err(Error::conflict(format!(
                "space already has the maximum of {} live nodes",
                limits.space_max_nodes
            )));
        }
        if delta.content_bytes > 0 && projected_content_bytes > max_content_bytes {
            return Err(Error::conflict(format!(
                "space content would exceed the maximum of {} bytes; delete, move, or split content",
                limits.space_max_content_bytes
            )));
        }
    } else if delta.nodes > 0 || delta.content_bytes > 0 {
        return Err(Error::internal(
            "positive usage delta requires quota limits",
        ));
    }

    let result = sqlx::query(
        "UPDATE space_usage \
         SET live_node_count = $2, live_content_bytes = $3 \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .bind(projected_nodes)
    .bind(projected_content_bytes)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    if result.rows_affected() != 1 {
        return Err(Error::internal("space usage counter update was lost"));
    }

    Ok(())
}
