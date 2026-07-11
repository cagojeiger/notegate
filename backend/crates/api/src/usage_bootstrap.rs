//! Startup validation and repair for authoritative Space usage counters.

use std::time::Duration;

use notegate_core::{Error, Result};
use notegate_db::{FullUsageReconcileRun, PgPool, SpaceUsageRepo};

// A peer may hold the worker lock for the full five-minute recalculation timeout.
const MAX_ATTEMPTS: usize = 330;
const RETRY_DELAY: Duration = Duration::from_secs(1);

/// Ensure every live Space has a counter before the API starts accepting traffic.
pub async fn ensure(pool: &PgPool) -> Result<()> {
    let repo = SpaceUsageRepo::new(pool.clone());
    repo.require_schema().await?;
    if !repo.has_missing_live_counters().await? {
        return Ok(());
    }

    tracing::warn!(event = "usage_bootstrap.missing_counters");
    for attempt in 1..=MAX_ATTEMPTS {
        if !repo.has_missing_live_counters().await? {
            tracing::info!(event = "usage_bootstrap.repaired_by_peer");
            return Ok(());
        }

        match repo.run_full_recalculation().await? {
            FullUsageReconcileRun::Recalculated {
                spaces_recalculated,
            } => {
                if repo.has_missing_live_counters().await? {
                    return Err(Error::internal(
                        "usage bootstrap completed with missing counters",
                    ));
                }
                tracing::info!(event = "usage_bootstrap.recalculated", spaces_recalculated);
                return Ok(());
            }
            FullUsageReconcileRun::WorkerLockHeld | FullUsageReconcileRun::MutationsActive
                if attempt < MAX_ATTEMPTS =>
            {
                tokio::time::sleep(RETRY_DELAY).await;
            }
            FullUsageReconcileRun::WorkerLockHeld => {
                return Err(Error::internal(
                    "usage bootstrap remained blocked by another reconciliation worker",
                ));
            }
            FullUsageReconcileRun::MutationsActive => {
                return Err(Error::internal(
                    "usage bootstrap remained blocked by active file mutations",
                ));
            }
        }
    }

    Err(Error::internal("usage bootstrap did not complete"))
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use notegate_db::{PgPool, test_support::TestDb};
    use uuid::Uuid;

    use super::ensure;

    async fn insert_space(pool: &PgPool, name: &str) -> std::result::Result<Uuid, sqlx::Error> {
        let user_id: Uuid =
            sqlx::query_scalar("INSERT INTO accounts (kind) VALUES ('user') RETURNING id")
                .fetch_one(pool)
                .await?;
        sqlx::query("INSERT INTO users (id) VALUES ($1)")
            .bind(user_id)
            .execute(pool)
            .await?;
        sqlx::query_scalar("INSERT INTO spaces (owner_user_id, name) VALUES ($1, $2) RETURNING id")
            .bind(user_id)
            .bind(name)
            .fetch_one(pool)
            .await
    }

    #[tokio::test]
    async fn ensure_repairs_missing_live_space_counters()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let space_id = insert_space(&db.pool, "bootstrap-repair").await?;
        sqlx::query("DELETE FROM space_usage WHERE space_id = $1")
            .bind(space_id)
            .execute(&db.pool)
            .await?;

        ensure(&db.pool).await?;

        let usage: (i64, i64) = sqlx::query_as(
            "SELECT live_node_count, live_content_bytes FROM space_usage WHERE space_id = $1",
        )
        .bind(space_id)
        .fetch_one(&db.pool)
        .await?;
        assert_eq!(usage, (1, 0));
        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn ensure_does_not_recalculate_complete_counters()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let space_id = insert_space(&db.pool, "bootstrap-ready").await?;
        let reconciled_at: DateTime<Utc> = sqlx::query_scalar(
            "UPDATE space_usage SET reconciled_at = now() - interval '1 day' \
             WHERE space_id = $1 RETURNING reconciled_at",
        )
        .bind(space_id)
        .fetch_one(&db.pool)
        .await?;

        ensure(&db.pool).await?;

        let after: DateTime<Utc> =
            sqlx::query_scalar("SELECT reconciled_at FROM space_usage WHERE space_id = $1")
                .bind(space_id)
                .fetch_one(&db.pool)
                .await?;
        assert_eq!(after, reconciled_at);
        db.cleanup().await;
        Ok(())
    }
}
