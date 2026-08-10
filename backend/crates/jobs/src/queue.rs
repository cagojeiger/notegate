use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgListener;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    AttemptOutcome, ClaimFence, ClaimedJob, EnqueuedJob, JobFailure, JobFailureClass,
    JobQueueError, JobQueueResult, JobQueueSnapshot, JobStateCount, NewJob, RecoverySummary,
};

pub const BACKGROUND_JOB_NOTIFY_CHANNEL: &str = "notegate_background_jobs";

const MAX_ERROR_CODE_BYTES: usize = 128;
const MAX_ERROR_MESSAGE_BYTES: usize = 4096;

#[derive(Debug, Clone)]
pub struct JobQueue {
    pool: PgPool,
}

impl JobQueue {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn connect_listener(&self) -> JobQueueResult<PgListener> {
        let mut listener = PgListener::connect_with(&self.pool).await?;
        listener.listen(BACKGROUND_JOB_NOTIFY_CHANNEL).await?;
        Ok(listener)
    }

    pub async fn enqueue(&self, job: &NewJob) -> JobQueueResult<EnqueuedJob> {
        let mut connection = self.pool.acquire().await?;
        Self::enqueue_in(&mut connection, job).await
    }

    pub async fn enqueue_in(
        connection: &mut PgConnection,
        job: &NewJob,
    ) -> JobQueueResult<EnqueuedJob> {
        validate_new_job(job)?;
        let job_id = sqlx::query_scalar("SELECT enqueue_background_job($1, $2, $3, $4)")
            .bind(&job.kind)
            .bind(&job.payload)
            .bind(job.available_at)
            .bind(job.max_attempts)
            .fetch_one(connection)
            .await?;
        Ok(EnqueuedJob { job_id })
    }

    pub async fn claim_many(
        &self,
        worker_id: &str,
        job_kinds: &[String],
        lease: Duration,
        limit: usize,
    ) -> JobQueueResult<Vec<ClaimedJob>> {
        if limit == 0 || job_kinds.is_empty() {
            return Ok(Vec::new());
        }
        if worker_id.is_empty() || worker_id.len() > 256 {
            return Err(JobQueueError::InvalidConfiguration(
                "worker id must contain between 1 and 256 bytes".to_owned(),
            ));
        }
        if lease.is_zero() {
            return Err(JobQueueError::InvalidConfiguration(
                "claim lease must be positive".to_owned(),
            ));
        }
        let limit = i64::try_from(limit).map_err(|_error| {
            JobQueueError::InvalidConfiguration("claim limit is too large".to_owned())
        })?;
        let mut tx = self.pool.begin().await?;
        let candidates = sqlx::query_as::<_, ClaimCandidate>(
            "SELECT job_id, job_kind, payload, attempt_count, failure_count, max_attempts, created_at \
             FROM background_jobs \
             WHERE status = 'queued' AND available_at <= now() \
               AND attempt_count < max_attempts \
               AND job_kind = ANY($2) \
             ORDER BY available_at, created_at, job_id \
             LIMIT $1 FOR UPDATE SKIP LOCKED",
        )
        .bind(limit)
        .bind(job_kinds)
        .fetch_all(&mut *tx)
        .await?;
        if candidates.is_empty() {
            tx.commit().await?;
            return Ok(Vec::new());
        }

        let job_ids = candidates
            .iter()
            .map(|candidate| candidate.job_id)
            .collect::<Vec<_>>();
        let claim_tokens = candidates
            .iter()
            .map(|_candidate| Uuid::new_v4())
            .collect::<Vec<_>>();
        sqlx::query(
            "UPDATE background_jobs job \
             SET status = 'running', attempt_count = job.attempt_count + 1, \
                 claim_token = claims.claim_token, claimed_by = $3, \
                 lease_until = now() + make_interval(secs => $4), \
                 updated_at = now() \
             FROM unnest($1::uuid[], $2::uuid[]) AS claims(job_id, claim_token) \
             WHERE job.job_id = claims.job_id",
        )
        .bind(&job_ids)
        .bind(&claim_tokens)
        .bind(worker_id)
        .bind(lease.as_secs_f64())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO background_job_attempts ( \
                 job_id, attempt_number, claim_token, worker_id, started_at \
             ) \
             SELECT job_id, attempt_count, claim_token, claimed_by, now() \
             FROM background_jobs WHERE job_id = ANY($1)",
        )
        .bind(&job_ids)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(candidates
            .into_iter()
            .zip(claim_tokens)
            .map(|(candidate, claim_token)| ClaimedJob {
                job_id: candidate.job_id,
                kind: candidate.job_kind,
                payload: candidate.payload,
                attempt: candidate.attempt_count + 1,
                failure_count: candidate.failure_count,
                max_attempts: candidate.max_attempts,
                claim_token,
                created_at: candidate.created_at,
            })
            .collect())
    }

    pub async fn heartbeat(&self, claim: &ClaimedJob, lease: Duration) -> JobQueueResult<bool> {
        let updated = sqlx::query(
            "UPDATE background_jobs \
             SET lease_until = now() + make_interval(secs => $3), updated_at = now() \
             WHERE job_id = $1 AND status = 'running' AND claim_token = $2 \
               AND lease_until > now()",
        )
        .bind(claim.job_id)
        .bind(claim.claim_token)
        .bind(lease.as_secs_f64())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(updated > 0)
    }

    pub async fn succeed(&self, claim: &ClaimedJob) -> JobQueueResult<bool> {
        let mut tx = self.pool.begin().await?;
        if !Self::owns_claim_in(&mut tx, &claim.fence()).await? {
            tx.commit().await?;
            return Ok(false);
        }
        finish_attempt(&mut tx, claim, "succeeded", None, None).await?;
        sqlx::query(
            "UPDATE background_jobs \
             SET status = 'succeeded', claim_token = NULL, claimed_by = NULL, \
                 lease_until = NULL, last_error_code = NULL, last_error_message = NULL, \
                 completed_at = now(), updated_at = now() \
             WHERE job_id = $1 AND claim_token = $2",
        )
        .bind(claim.job_id)
        .bind(claim.claim_token)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn defer(
        &self,
        claim: &ClaimedJob,
        reason: &str,
        retry_delay: Duration,
    ) -> JobQueueResult<DeferTransition> {
        let mut tx = self.pool.begin().await?;
        if !Self::owns_claim_in(&mut tx, &claim.fence()).await? {
            tx.commit().await?;
            return Ok(DeferTransition::ClaimLost);
        }
        let reason = bounded_error_code(reason);
        finish_attempt(&mut tx, claim, "deferred", Some(&reason), None).await?;
        let exhausted = attempts_exhausted(claim.attempt, claim.max_attempts);
        if exhausted {
            sqlx::query(
                "UPDATE background_jobs \
                 SET status = 'dead', claim_token = NULL, claimed_by = NULL, \
                     lease_until = NULL, last_error_code = $3, \
                     last_error_message = 'job deferred on its final attempt', \
                     completed_at = now(), updated_at = now() \
                 WHERE job_id = $1 AND claim_token = $2",
            )
            .bind(claim.job_id)
            .bind(claim.claim_token)
            .bind(&reason)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                "UPDATE background_jobs \
                 SET status = 'queued', available_at = now() + make_interval(secs => $3), \
                     claim_token = NULL, claimed_by = NULL, lease_until = NULL, \
                     last_error_code = $4, last_error_message = NULL, \
                     completed_at = NULL, updated_at = now() \
                 WHERE job_id = $1 AND claim_token = $2",
            )
            .bind(claim.job_id)
            .bind(claim.claim_token)
            .bind(retry_delay.as_secs_f64())
            .bind(&reason)
            .execute(&mut *tx)
            .await?;
            sqlx::query("SELECT pg_notify($1, $2)")
                .bind(BACKGROUND_JOB_NOTIFY_CHANNEL)
                .bind(&claim.kind)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(if exhausted {
            DeferTransition::Dead
        } else {
            DeferTransition::Deferred
        })
    }

    pub async fn fail(
        &self,
        claim: &ClaimedJob,
        failure: &JobFailure,
        outcome: AttemptOutcome,
        retry_delay: Duration,
    ) -> JobQueueResult<FailureTransition> {
        let mut tx = self.pool.begin().await?;
        if !Self::owns_claim_in(&mut tx, &claim.fence()).await? {
            tx.commit().await?;
            return Ok(FailureTransition::ClaimLost);
        }
        let code = bounded_error_code(&failure.code);
        let message = bounded(&failure.message, MAX_ERROR_MESSAGE_BYTES);
        let terminal = failure_is_terminal(failure.class, claim.attempt, claim.max_attempts);
        finish_attempt(
            &mut tx,
            claim,
            outcome.as_str(),
            Some(&code),
            Some(&message),
        )
        .await?;
        if terminal {
            sqlx::query(
                "UPDATE background_jobs job \
                 SET status = 'dead', failure_count = job.failure_count + 1, \
                     claim_token = NULL, claimed_by = NULL, \
                     lease_until = NULL, last_error_code = $3, last_error_message = $4, \
                     completed_at = now(), updated_at = now() \
                 WHERE job_id = $1 AND claim_token = $2",
            )
            .bind(claim.job_id)
            .bind(claim.claim_token)
            .bind(&code)
            .bind(&message)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                "UPDATE background_jobs job \
                 SET status = 'queued', failure_count = job.failure_count + 1, \
                     available_at = now() + make_interval(secs => $3), \
                     claim_token = NULL, claimed_by = NULL, lease_until = NULL, \
                     last_error_code = $4, last_error_message = $5, \
                     completed_at = NULL, updated_at = now() \
                 WHERE job_id = $1 AND claim_token = $2",
            )
            .bind(claim.job_id)
            .bind(claim.claim_token)
            .bind(retry_delay.as_secs_f64())
            .bind(&code)
            .bind(&message)
            .execute(&mut *tx)
            .await?;
            sqlx::query("SELECT pg_notify($1, $2)")
                .bind(BACKGROUND_JOB_NOTIFY_CHANNEL)
                .bind(&claim.kind)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(if terminal {
            FailureTransition::Dead
        } else {
            FailureTransition::Retrying
        })
    }

    pub async fn recover_expired(&self, limit: usize) -> JobQueueResult<RecoverySummary> {
        if limit == 0 {
            return Ok(RecoverySummary::default());
        }
        let limit = i64::try_from(limit).map_err(|_error| {
            JobQueueError::InvalidConfiguration("recovery limit is too large".to_owned())
        })?;
        let mut tx = self.pool.begin().await?;
        let summary = recover_expired_in(&mut tx, limit).await?;
        tx.commit().await?;
        Ok(summary)
    }

    pub(crate) async fn try_recover_expired(
        &self,
        limit: usize,
    ) -> JobQueueResult<Option<RecoverySummary>> {
        if limit == 0 {
            return Ok(Some(RecoverySummary::default()));
        }
        let limit = i64::try_from(limit).map_err(|_error| {
            JobQueueError::InvalidConfiguration("recovery limit is too large".to_owned())
        })?;
        let mut tx = self.pool.begin().await?;
        if !try_reconciler_lock(&mut tx).await? {
            tx.commit().await?;
            return Ok(None);
        }
        let summary = recover_expired_in(&mut tx, limit).await?;
        tx.commit().await?;
        Ok(Some(summary))
    }

    pub async fn purge_completed(&self, retention: Duration, limit: usize) -> JobQueueResult<u64> {
        if limit == 0 {
            return Ok(0);
        }
        let limit = i64::try_from(limit).map_err(|_error| {
            JobQueueError::InvalidConfiguration("purge limit is too large".to_owned())
        })?;
        let mut connection = self.pool.acquire().await?;
        purge_completed_in(&mut connection, retention, limit).await
    }

    pub(crate) async fn try_purge_completed(
        &self,
        retention: Duration,
        limit: usize,
    ) -> JobQueueResult<Option<u64>> {
        if limit == 0 {
            return Ok(Some(0));
        }
        let limit = i64::try_from(limit).map_err(|_error| {
            JobQueueError::InvalidConfiguration("purge limit is too large".to_owned())
        })?;
        let mut tx = self.pool.begin().await?;
        if !try_reconciler_lock(&mut tx).await? {
            tx.commit().await?;
            return Ok(None);
        }
        let deleted = purge_completed_in(&mut tx, retention, limit).await?;
        tx.commit().await?;
        Ok(Some(deleted))
    }

    pub async fn next_wake_delay(
        &self,
        job_kinds: &[String],
        maximum: Duration,
    ) -> JobQueueResult<Duration> {
        if job_kinds.is_empty() {
            return Ok(maximum);
        }
        let delay_seconds: Option<f64> = sqlx::query_scalar(
            "SELECT CASE WHEN min(available_at) IS NULL THEN NULL \
                         ELSE GREATEST( \
                             0::double precision, \
                             EXTRACT(EPOCH FROM (min(available_at) - now()))::double precision \
                         ) \
                    END \
             FROM background_jobs \
             WHERE status = 'queued' AND attempt_count < max_attempts \
               AND job_kind = ANY($1)",
        )
        .bind(job_kinds)
        .fetch_one(&self.pool)
        .await?;
        let Some(delay_seconds) = delay_seconds else {
            return Ok(maximum);
        };
        let delay = Duration::from_secs_f64(delay_seconds);
        Ok(delay.min(maximum))
    }

    pub async fn snapshot(&self, job_kinds: &[String]) -> JobQueueResult<JobQueueSnapshot> {
        let states = sqlx::query_as::<_, StateCountRow>(
            "SELECT job_kind, \
                    CASE \
                        WHEN status = 'queued' AND available_at <= now() THEN 'ready' \
                        WHEN status = 'queued' THEN 'delayed' \
                        WHEN status = 'running' AND lease_until <= now() THEN 'lease_expired' \
                        ELSE status \
                    END AS state, \
                    count(*)::bigint AS count \
             FROM background_jobs \
             WHERE job_kind = ANY($1) AND status IN ('queued', 'running', 'dead') \
             GROUP BY job_kind, state \
             ORDER BY job_kind, state",
        )
        .bind(job_kinds)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| JobStateCount {
            kind: row.job_kind,
            state: row.state,
            count: row.count,
        })
        .collect();
        let oldest_ready_at = sqlx::query_scalar(
            "SELECT min(available_at) FROM background_jobs \
             WHERE status = 'queued' AND available_at <= now() \
               AND job_kind = ANY($1)",
        )
        .bind(job_kinds)
        .fetch_one(&self.pool)
        .await?;
        Ok(JobQueueSnapshot {
            states,
            oldest_ready_at,
        })
    }

    /// Lock the current claim inside a domain transaction.
    ///
    /// Keep the transaction open until its side effects commit so an expired
    /// claim cannot race a replacement worker and publish a late result.
    pub async fn owns_claim_in(
        connection: &mut PgConnection,
        fence: &ClaimFence,
    ) -> JobQueueResult<bool> {
        let owned = sqlx::query_scalar(
            "SELECT true FROM background_jobs \
             WHERE job_id = $1 AND status = 'running' AND claim_token = $2 \
               AND lease_until > now() FOR UPDATE",
        )
        .bind(fence.job_id)
        .bind(fence.claim_token)
        .fetch_optional(connection)
        .await?;
        Ok(owned.unwrap_or(false))
    }
}

async fn try_reconciler_lock(connection: &mut PgConnection) -> JobQueueResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT try_lock_background_job_reconciler()")
            .fetch_one(connection)
            .await?,
    )
}

async fn recover_expired_in(
    connection: &mut PgConnection,
    limit: i64,
) -> JobQueueResult<RecoverySummary> {
    let expired = sqlx::query_as::<_, ExpiredClaim>(
        "SELECT job_id, job_kind, claim_token, attempt_count, max_attempts \
             FROM background_jobs \
             WHERE status = 'running' AND lease_until <= now() \
             ORDER BY lease_until, job_id \
             LIMIT $1 FOR UPDATE SKIP LOCKED",
    )
    .bind(limit)
    .fetch_all(&mut *connection)
    .await?;
    let mut summary = RecoverySummary::default();
    for claim in expired {
        sqlx::query(
            "UPDATE background_job_attempts \
                 SET finished_at = now(), outcome = 'lease_expired', \
                     error_code = 'lease_expired', error_message = 'worker lease expired' \
                 WHERE job_id = $1 AND claim_token = $2 AND finished_at IS NULL",
        )
        .bind(claim.job_id)
        .bind(claim.claim_token)
        .execute(&mut *connection)
        .await?;
        if attempts_exhausted(claim.attempt_count, claim.max_attempts) {
            sqlx::query(
                "UPDATE background_jobs job \
                     SET status = 'dead', failure_count = job.failure_count + 1, \
                         claim_token = NULL, claimed_by = NULL, \
                         lease_until = NULL, last_error_code = 'lease_expired', \
                         last_error_message = 'worker lease expired', \
                         completed_at = now(), updated_at = now() \
                     WHERE job_id = $1 AND claim_token = $2",
            )
            .bind(claim.job_id)
            .bind(claim.claim_token)
            .execute(&mut *connection)
            .await?;
            summary.dead += 1;
        } else {
            sqlx::query(
                "UPDATE background_jobs job \
                     SET status = 'queued', failure_count = job.failure_count + 1, \
                         available_at = now(), \
                         claim_token = NULL, claimed_by = NULL, lease_until = NULL, \
                         last_error_code = 'lease_expired', \
                         last_error_message = 'worker lease expired', \
                         completed_at = NULL, updated_at = now() \
                     WHERE job_id = $1 AND claim_token = $2",
            )
            .bind(claim.job_id)
            .bind(claim.claim_token)
            .execute(&mut *connection)
            .await?;
            summary.retried += 1;
        }
        tracing::warn!(
            event = "background_job.lease_expired",
            job_kind = claim.job_kind,
            job_id = %claim.job_id,
        );
    }
    if summary.retried > 0 {
        sqlx::query("SELECT pg_notify($1, 'lease_recovered')")
            .bind(BACKGROUND_JOB_NOTIFY_CHANNEL)
            .execute(&mut *connection)
            .await?;
    }
    Ok(summary)
}

async fn purge_completed_in(
    connection: &mut PgConnection,
    retention: Duration,
    limit: i64,
) -> JobQueueResult<u64> {
    let deleted = sqlx::query(
        "WITH expired AS ( \
                 SELECT job_id FROM background_jobs \
                 WHERE status IN ('succeeded', 'dead') \
                   AND completed_at < now() - make_interval(secs => $1) \
                 ORDER BY completed_at, job_id \
                 LIMIT $2 FOR UPDATE SKIP LOCKED \
             ) \
             DELETE FROM background_jobs job USING expired \
             WHERE job.job_id = expired.job_id",
    )
    .bind(retention.as_secs_f64())
    .bind(limit)
    .execute(connection)
    .await?
    .rows_affected();
    Ok(deleted)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureTransition {
    Retrying,
    Dead,
    ClaimLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferTransition {
    Deferred,
    Dead,
    ClaimLost,
}

async fn finish_attempt(
    connection: &mut PgConnection,
    claim: &ClaimedJob,
    outcome: &str,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> JobQueueResult<()> {
    sqlx::query(
        "UPDATE background_job_attempts \
         SET finished_at = now(), outcome = $3, error_code = $4, error_message = $5 \
         WHERE job_id = $1 AND claim_token = $2 AND finished_at IS NULL",
    )
    .bind(claim.job_id)
    .bind(claim.claim_token)
    .bind(outcome)
    .bind(error_code)
    .bind(error_message)
    .execute(connection)
    .await?;
    Ok(())
}

fn validate_new_job(job: &NewJob) -> JobQueueResult<()> {
    validate_job_kind(&job.kind)?;
    if !(1..=100).contains(&job.max_attempts) {
        return Err(JobQueueError::InvalidConfiguration(
            "max attempts must be between 1 and 100".to_owned(),
        ));
    }
    Ok(())
}

fn failure_is_terminal(class: JobFailureClass, attempt_count: i32, max_attempts: i32) -> bool {
    class == JobFailureClass::Permanent || attempts_exhausted(attempt_count, max_attempts)
}

fn attempts_exhausted(attempt_count: i32, max_attempts: i32) -> bool {
    attempt_count >= max_attempts
}

pub(crate) fn validate_job_kind(kind: &str) -> JobQueueResult<()> {
    if kind.is_empty() || kind.len() > 128 {
        return Err(JobQueueError::InvalidConfiguration(
            "job kind must contain between 1 and 128 bytes".to_owned(),
        ));
    }
    Ok(())
}

fn bounded(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn bounded_error_code(value: &str) -> String {
    if value.is_empty() {
        "job_failed".to_owned()
    } else {
        bounded(value, MAX_ERROR_CODE_BYTES)
    }
}

#[derive(Debug, FromRow)]
struct ClaimCandidate {
    job_id: Uuid,
    job_kind: String,
    payload: Value,
    attempt_count: i32,
    failure_count: i32,
    max_attempts: i32,
    created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct ExpiredClaim {
    job_id: Uuid,
    job_kind: String,
    claim_token: Uuid,
    attempt_count: i32,
    max_attempts: i32,
}

#[derive(Debug, FromRow)]
struct StateCountRow {
    job_kind: String,
    state: String,
    count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_error_text_preserves_utf8_boundaries() {
        assert_eq!(bounded("가나다", 4), "가");
        assert_eq!(bounded("small", 8), "small");
    }

    #[test]
    fn new_job_validation_rejects_invalid_attempt_limits() {
        let error = validate_new_job(&NewJob::new("test", Value::Null).max_attempts(0));
        assert!(matches!(error, Err(JobQueueError::InvalidConfiguration(_))));
    }

    #[test]
    fn empty_failure_codes_have_a_valid_fallback() {
        assert_eq!(bounded_error_code(""), "job_failed");
    }

    #[test]
    fn every_retryable_job_becomes_terminal_at_its_attempt_limit() {
        for max_attempts in 1..=100 {
            for attempt in 1..=max_attempts {
                assert_eq!(
                    failure_is_terminal(JobFailureClass::Retryable, attempt, max_attempts,),
                    attempt == max_attempts,
                );
            }
        }
    }

    #[test]
    fn permanent_failures_are_always_terminal() {
        for max_attempts in 1..=100 {
            assert!(failure_is_terminal(
                JobFailureClass::Permanent,
                1,
                max_attempts,
            ));
        }
    }
}
