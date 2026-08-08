//! Durable desired-state work claims shared by reconciliation workers.

use std::time::Duration;

use notegate_core::Result;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

#[derive(Debug, Clone)]
pub struct ReconciliationRepo {
    pool: PgPool,
}

impl ReconciliationRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn claim_one(
        &self,
        queue_name: &str,
        lease: Duration,
    ) -> Result<Option<ReconciliationClaim>> {
        let claim_token = Uuid::new_v4();
        sqlx::query_as(
            "WITH candidate AS ( \
                SELECT work_kind, target_id, requested_generation \
                FROM reconciliation_work_items \
                WHERE queue_name = $1 \
                  AND requested_generation > applied_generation \
                  AND run_after <= now() \
                  AND (lease_until IS NULL OR lease_until <= now()) \
                ORDER BY run_after, created_at, target_id \
                LIMIT 1 FOR UPDATE SKIP LOCKED \
             ) \
             UPDATE reconciliation_work_items work \
             SET claimed_generation = candidate.requested_generation, \
                 claim_token = $2, \
                 lease_until = now() + make_interval(secs => $3), \
                 updated_at = now() \
             FROM candidate \
             WHERE work.queue_name = $1 \
               AND work.work_kind = candidate.work_kind \
               AND work.target_id = candidate.target_id \
             RETURNING work.queue_name, work.work_kind, work.space_id, \
                       work.target_id, candidate.requested_generation, \
                       work.claim_token",
        )
        .bind(queue_name)
        .bind(claim_token)
        .bind(lease.as_secs_f64())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn complete_in(
        connection: &mut sqlx::PgConnection,
        claim: &ReconciliationClaim,
    ) -> Result<bool> {
        let updated = sqlx::query(
            "UPDATE reconciliation_work_items \
             SET applied_generation = claimed_generation, \
                 claimed_generation = NULL, claim_token = NULL, lease_until = NULL, \
                 attempt_count = 0, last_error = NULL, last_completed_at = now(), \
                 updated_at = now() \
             WHERE queue_name = $1 AND work_kind = $2 \
               AND target_id = $3 AND claim_token = $4",
        )
        .bind(&claim.queue_name)
        .bind(&claim.work_kind)
        .bind(claim.target_id)
        .bind(claim.claim_token)
        .execute(connection)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected();
        Ok(updated > 0)
    }

    pub async fn owns_claim_in(
        connection: &mut sqlx::PgConnection,
        claim: &ReconciliationClaim,
    ) -> Result<bool> {
        sqlx::query_scalar(
            "SELECT true FROM reconciliation_work_items \
             WHERE queue_name = $1 AND work_kind = $2 \
               AND target_id = $3 AND claim_token = $4 \
             FOR UPDATE",
        )
        .bind(&claim.queue_name)
        .bind(&claim.work_kind)
        .bind(claim.target_id)
        .bind(claim.claim_token)
        .fetch_optional(connection)
        .await
        .map(|owned| owned.unwrap_or(false))
        .map_err(map_sqlx_error)
    }

    pub async fn delete_in(
        connection: &mut sqlx::PgConnection,
        claim: &ReconciliationClaim,
    ) -> Result<bool> {
        let deleted = sqlx::query(
            "DELETE FROM reconciliation_work_items \
             WHERE queue_name = $1 AND work_kind = $2 \
               AND target_id = $3 AND claim_token = $4",
        )
        .bind(&claim.queue_name)
        .bind(&claim.work_kind)
        .bind(claim.target_id)
        .bind(claim.claim_token)
        .execute(connection)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected();
        Ok(deleted > 0)
    }

    pub async fn fail(
        &self,
        claim: &ReconciliationClaim,
        retry_delay: Duration,
        error: &str,
    ) -> Result<bool> {
        let updated = sqlx::query(
            "UPDATE reconciliation_work_items \
             SET claimed_generation = NULL, claim_token = NULL, lease_until = NULL, \
                 run_after = CASE \
                     WHEN requested_generation > claimed_generation THEN now() \
                     ELSE now() + make_interval(secs => $5) \
                 END, \
                 attempt_count = attempt_count + 1, last_error = $6, updated_at = now() \
             WHERE queue_name = $1 AND work_kind = $2 \
               AND target_id = $3 AND claim_token = $4",
        )
        .bind(&claim.queue_name)
        .bind(&claim.work_kind)
        .bind(claim.target_id)
        .bind(claim.claim_token)
        .bind(retry_delay.as_secs_f64())
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected();
        Ok(updated > 0)
    }

    pub async fn backlog(&self, queue_name: &str) -> Result<i64> {
        sqlx::query_scalar("SELECT reconciliation_backlog($1)")
            .bind(queue_name)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx_error)
    }
}

#[derive(Debug, Clone, FromRow, PartialEq, Eq)]
pub struct ReconciliationClaim {
    pub queue_name: String,
    pub work_kind: String,
    pub space_id: Uuid,
    pub target_id: Uuid,
    pub requested_generation: i64,
    pub claim_token: Uuid,
}
