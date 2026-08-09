//! Exact reconciliation for transactionally maintained Space usage counters.

mod reconciliation;

use notegate_core::{Error, Result};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

pub use reconciliation::UsageReconcileResult;

const LOCK_TIMEOUT: &str = "5s";
pub const SPACE_USAGE_JOB_KIND: &str = "space_usage_reconcile";

#[derive(Debug, Clone)]
pub struct SpaceUsageRepo {
    pool: PgPool,
}

impl SpaceUsageRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Require the usage tables, split byte counters, and Space creation trigger.
    pub async fn require_schema(&self) -> Result<()> {
        let installed: bool = sqlx::query_scalar(
            "SELECT EXISTS ( \
                        SELECT 1 FROM pg_class c \
                        JOIN pg_namespace n ON n.oid = c.relnamespace \
                        WHERE n.nspname = current_schema() \
                          AND c.relname = 'space_usage' AND c.relkind = 'r' \
                    ) \
                    AND EXISTS ( \
                        SELECT 1 FROM pg_class c \
                        JOIN pg_namespace n ON n.oid = c.relnamespace \
                        WHERE n.nspname = current_schema() \
                          AND c.relname = 'background_jobs' AND c.relkind = 'r' \
                    ) \
                    AND EXISTS ( \
                        SELECT 1 FROM pg_trigger t \
                        JOIN pg_class c ON c.oid = t.tgrelid \
                        JOIN pg_namespace n ON n.oid = c.relnamespace \
                        WHERE n.nspname = current_schema() AND c.relname = 'spaces' \
                          AND t.tgname = 'spaces_create_usage' \
                          AND NOT t.tgisinternal \
                          AND t.tgenabled IN ('O', 'A') \
                    ) \
                    AND EXISTS ( \
                        SELECT 1 FROM information_schema.columns \
                        WHERE table_schema = current_schema() AND table_name = 'space_usage' \
                          AND column_name = 'live_text_bytes' \
                    ) \
                    AND EXISTS ( \
                        SELECT 1 FROM information_schema.columns \
                        WHERE table_schema = current_schema() AND table_name = 'space_usage' \
                          AND column_name = 'live_file_bytes' \
                    )",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        if !installed {
            return Err(Error::internal(
                "required space usage schema is not installed",
            ));
        }
        Ok(())
    }

    /// Return whether any live Space is missing its authoritative counter row.
    pub async fn has_missing_live_counters(&self) -> Result<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS ( \
                 SELECT 1 FROM spaces s \
                 WHERE s.deleted_at IS NULL \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM space_usage su WHERE su.space_id = s.id \
                   ) \
             )",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    /// Calculate source-of-truth usage for diagnostics and reconciliation.
    /// Quota checks must use the locked counter instead of this full scan.
    pub async fn calculate_exact_usage(&self, space_id: Uuid) -> Result<UsageCounts> {
        let mut connection = self.pool.acquire().await.map_err(map_sqlx_error)?;
        exact_usage(&mut connection, space_id).await
    }

    /// Explicit operator repair. Background requests use the generic queue;
    /// this synchronous path returns only after every live Space was visited.
    pub async fn reconcile_all_live_spaces(&self) -> Result<u64> {
        let space_ids: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM spaces WHERE deleted_at IS NULL ORDER BY id")
                .fetch_all(&self.pool)
                .await
                .map_err(map_sqlx_error)?;
        for space_id in &space_ids {
            match self.reconcile_space(*space_id).await? {
                UsageReconcileResult::Reconciled { .. } | UsageReconcileResult::Deleted => {}
                UsageReconcileResult::Busy => {
                    return Err(Error::usage_recalculation_in_progress(2));
                }
            }
        }
        u64::try_from(space_ids.len())
            .map_err(|_error| Error::internal("live Space count is too large"))
    }
}

async fn configure_transaction(tx: &mut sqlx::PgConnection, statement_timeout: &str) -> Result<()> {
    sqlx::query(
        "SELECT set_config('lock_timeout', $1, true), \
                set_config('statement_timeout', $2, true)",
    )
    .bind(LOCK_TIMEOUT)
    .bind(statement_timeout)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

/// SELECT columns computing live usage for the Space referenced by `space_ref`.
/// The single formulation of what counts toward usage; per-space
/// reconciliation and the full recalculation must not drift apart.
fn live_usage_columns(space_ref: &'static str) -> String {
    format!(
        "(SELECT count(*) FROM nodes n \
          WHERE n.space_id = {space_ref} AND n.deleted_at IS NULL) AS live_node_count, \
         COALESCE(( \
             SELECT sum(t.byte_len) FROM text_objects t \
             JOIN nodes n ON n.id = t.node_id AND n.space_id = t.space_id \
             WHERE t.space_id = {space_ref} AND n.deleted_at IS NULL \
         ), 0)::bigint AS live_text_bytes, \
         COALESCE(( \
             SELECT sum(f.byte_len) FROM file_objects f \
             JOIN nodes n ON n.id = f.node_id AND n.space_id = f.space_id \
             WHERE f.space_id = {space_ref} AND n.deleted_at IS NULL \
         ), 0)::bigint AS live_file_bytes"
    )
}

async fn exact_usage(tx: &mut sqlx::PgConnection, space_id: Uuid) -> Result<UsageCounts> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {}",
        live_usage_columns("$1")
    )))
    .bind(space_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)
}

#[derive(Debug, Clone, Copy, FromRow, PartialEq, Eq)]
pub struct UsageCounts {
    pub live_node_count: i64,
    pub live_text_bytes: i64,
    pub live_file_bytes: i64,
}
