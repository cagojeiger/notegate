//! Startup validation and operator repair for authoritative Space usage counters.

use std::time::Duration;

use notegate_core::{Error, Result};
use notegate_db::{FullUsageReconcileExecution, PgPool, SpaceUsageRepo};

const OPERATOR_MAX_POLLS: usize = 30;
const RETRY_DELAY: Duration = Duration::from_secs(1);

/// Ensure every live Space has a counter before the API starts accepting traffic.
pub async fn ensure(pool: &PgPool) -> Result<()> {
    let repo = SpaceUsageRepo::new(pool.clone());
    repo.require_schema().await?;
    if repo.has_missing_live_counters().await? {
        return Err(Error::internal(
            "live space is missing its usage counter; run the operator recalculation",
        ));
    }
    Ok(())
}

/// Rebuild every live Space counter for an explicit operator maintenance run.
pub async fn recalculate_all(pool: &PgPool) -> Result<u64> {
    let repo = SpaceUsageRepo::new(pool.clone());
    repo.require_schema().await?;
    for poll in 1..=OPERATOR_MAX_POLLS {
        match repo.execute_full_recalculation().await? {
            FullUsageReconcileExecution::Recalculated {
                spaces_recalculated,
            } => return Ok(spaces_recalculated),
            FullUsageReconcileExecution::WorkerLockHeld
            | FullUsageReconcileExecution::MutationsActive
                if poll < OPERATOR_MAX_POLLS =>
            {
                tokio::time::sleep(RETRY_DELAY).await;
            }
            FullUsageReconcileExecution::WorkerLockHeld => {
                return Err(Error::internal(
                    "usage reconciliation remained busy; retry later",
                ));
            }
            FullUsageReconcileExecution::MutationsActive => {
                return Err(Error::internal(
                    "file-tree mutations remained active; retry later",
                ));
            }
        }
    }

    Err(Error::internal("full usage recalculation did not complete"))
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
    async fn ensure_rejects_missing_live_space_counters()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let space_id = insert_space(&db.pool, "bootstrap-repair").await?;
        sqlx::query("DELETE FROM space_usage WHERE space_id = $1")
            .bind(space_id)
            .execute(&db.pool)
            .await?;

        let error = match ensure(&db.pool).await {
            Ok(()) => return Err("missing counters must block startup".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("operator recalculation"));
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
