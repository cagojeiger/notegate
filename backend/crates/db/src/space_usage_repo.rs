//! Exact reconciliation for transactionally maintained Space usage counters.

mod maintenance;
mod reconciliation;

use notegate_core::{Error, Result};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{map_sqlx_error, space_usage};

pub use maintenance::FullUsageReconcileExecution;
pub use reconciliation::UsageReconcileExecution;

const RECONCILE_ADVISORY_LOCK_SEED: i64 = 0x4e47_5553_4147_4501;
const LOCK_TIMEOUT: &str = "5s";

#[derive(Debug, Clone)]
pub struct SpaceUsageRepo {
    pool: PgPool,
}

impl SpaceUsageRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Require the usage tables and active Space creation trigger from migration 0012.
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
                          AND c.relname = 'space_usage_reconcile_jobs' AND c.relkind = 'r' \
                    ) \
                    AND EXISTS ( \
                        SELECT 1 FROM pg_class c \
                        JOIN pg_namespace n ON n.oid = c.relnamespace \
                        WHERE n.nspname = current_schema() \
                          AND c.relname = 'space_usage_reconcile_executions' AND c.relkind = 'r' \
                    ) \
                    AND EXISTS ( \
                        SELECT 1 FROM pg_trigger t \
                        JOIN pg_class c ON c.oid = t.tgrelid \
                        JOIN pg_namespace n ON n.oid = c.relnamespace \
                        WHERE n.nspname = current_schema() AND c.relname = 'spaces' \
                          AND t.tgname = 'spaces_create_usage' \
                          AND NOT t.tgisinternal \
                          AND t.tgenabled IN ('O', 'A') \
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

async fn try_acquire_worker_lock(tx: &mut sqlx::PgConnection) -> Result<bool> {
    space_usage::try_schema_advisory_lock(tx, RECONCILE_ADVISORY_LOCK_SEED, false).await
}

async fn exact_usage(tx: &mut sqlx::PgConnection, space_id: Uuid) -> Result<UsageCounts> {
    sqlx::query_as(
        "SELECT \
             (SELECT count(*) FROM nodes \
              WHERE space_id = $1 AND deleted_at IS NULL) AS live_node_count, \
             COALESCE(( \
                 SELECT sum(t.byte_len) FROM text_objects t \
                 JOIN nodes n ON n.id = t.node_id AND n.space_id = t.space_id \
                 WHERE t.space_id = $1 AND n.deleted_at IS NULL \
             ), 0)::bigint + COALESCE(( \
                 SELECT sum(f.byte_len) FROM file_objects f \
                 JOIN nodes n ON n.id = f.node_id AND n.space_id = f.space_id \
                 WHERE f.space_id = $1 AND n.deleted_at IS NULL \
             ), 0)::bigint AS live_content_bytes",
    )
    .bind(space_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)
}

#[derive(Debug, Clone, Copy, FromRow, PartialEq, Eq)]
pub struct UsageCounts {
    pub live_node_count: i64,
    pub live_content_bytes: i64,
}
