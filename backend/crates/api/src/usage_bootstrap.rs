//! Startup validation and operator repair for authoritative Space usage counters.

use std::time::Duration;

use notegate_core::{Error, Result};
use notegate_db::{PgPool, SpaceUsageRepo, UsageReconcileExecution};

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

/// Queue every live Space and drain the reconciliation queue, one Space at a
/// time, for an explicit operator maintenance run. Ordinary writes keep
/// working; only the Space currently being reconciled is briefly rejected.
pub async fn recalculate_all(pool: &PgPool) -> Result<u64> {
    let repo = SpaceUsageRepo::new(pool.clone());
    repo.require_schema().await?;
    let queued = repo.enqueue_all_live_spaces().await?;

    let mut busy_polls = 0usize;
    loop {
        match repo.execute_next_reconciliation().await? {
            UsageReconcileExecution::Succeeded { .. } => busy_polls = 0,
            UsageReconcileExecution::Cancelled { .. } => busy_polls = 0,
            UsageReconcileExecution::Idle => return complete_if_drained(&repo, queued).await,
            UsageReconcileExecution::WorkerLockHeld if busy_polls < OPERATOR_MAX_POLLS => {
                busy_polls += 1;
                tokio::time::sleep(RETRY_DELAY).await;
            }
            UsageReconcileExecution::WorkerLockHeld => {
                return Err(Error::internal(
                    "usage reconciliation remained busy; retry later",
                ));
            }
            UsageReconcileExecution::Deferred { space_id, .. } => {
                return Err(Error::internal(format!(
                    "space {space_id} stayed busy; the background worker retries it, \
                     or re-run this command later"
                )));
            }
            UsageReconcileExecution::Failed {
                space_id, error, ..
            } => {
                return Err(Error::internal(format!(
                    "space {space_id} reconciliation failed: {error}"
                )));
            }
        }
    }
}

async fn complete_if_drained(repo: &SpaceUsageRepo, queued: u64) -> Result<u64> {
    if repo.has_pending_reconciliations().await? {
        return Err(Error::internal(
            "usage reconciliation queue still has pending jobs; retry later",
        ));
    }
    Ok(queued)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use notegate_db::{PgPool, SpaceUsageRepo, test_support::TestDb};
    use uuid::Uuid;

    use super::{complete_if_drained, ensure};

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

    #[tokio::test]
    async fn operator_completion_rejects_a_deferred_queue()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        insert_space(&db.pool, "operator-deferred").await?;
        let repo = SpaceUsageRepo::new(db.pool.clone());
        let queued = repo.enqueue_all_live_spaces().await?;
        sqlx::query(
            "UPDATE space_usage_reconcile_jobs \
             SET run_after = now() + interval '5 minutes'",
        )
        .execute(&db.pool)
        .await?;

        let error = match complete_if_drained(&repo, queued).await {
            Ok(_) => return Err("a deferred job must keep the operator run incomplete".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("pending jobs"));

        sqlx::query("DELETE FROM space_usage_reconcile_jobs")
            .execute(&db.pool)
            .await?;
        assert_eq!(complete_if_drained(&repo, queued).await?, queued);

        db.cleanup().await;
        Ok(())
    }
}
