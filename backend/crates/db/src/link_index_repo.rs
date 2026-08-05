//! Durable Space link-index queue and projection writes.

use std::time::Duration;

use chrono::{DateTime, Utc};
use notegate_core::{Error, Result};
use notegate_model::{FileChangeEvent, LinkIndexStatus, LinkReferenceKind, SpaceLinkIndexState};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::map_sqlx_error;

mod relations;

pub use relations::{LinkReferenceRecord, NodeLinkRecords};

const LINK_INSERT_CHUNK_SIZE: usize = 500;

#[derive(Debug, Clone)]
pub struct LinkIndexRepo {
    pool: PgPool,
}

#[derive(Debug, Clone)]
pub struct LinkIndexClaim {
    pub space_id: Uuid,
    pub token: Uuid,
    pub desired_generation: i64,
    pub applied_generation: i64,
    pub status: LinkIndexStatus,
    pub rebuild_requested: bool,
    pub rebuild_base_generation: Option<i64>,
    pub rebuild_after_node_id: Option<Uuid>,
    pub parser_version: i32,
    pub retry_count: i32,
}

#[derive(Debug)]
pub struct LinkIndexEventBatch {
    pub events: Vec<QueuedLinkIndexEvent>,
    pub cursor_valid: bool,
}

#[derive(Debug, Clone)]
pub struct QueuedLinkIndexEvent {
    pub generation: i64,
    pub event: FileChangeEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLinkReference {
    pub kind: LinkReferenceKind,
    pub raw_href: String,
    pub normalized_target_path: Option<String>,
    pub target_node_id: Option<Uuid>,
    pub occurrence_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLinkSet {
    pub source_node_id: Uuid,
    pub references: Vec<NewLinkReference>,
}

impl LinkIndexRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn state(&self, space_id: Uuid) -> Result<Option<SpaceLinkIndexState>> {
        let row = sqlx::query_as::<_, LinkIndexStateRow>(
            "SELECT space_id, desired_generation, applied_generation, status, last_indexed_at \
             FROM space_link_index_states WHERE space_id = $1",
        )
        .bind(space_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.map(LinkIndexStateRow::into_public).transpose()
    }

    pub async fn newest_parser_version(&self) -> Result<Option<i32>> {
        sqlx::query_scalar("SELECT max(parser_version) FROM space_link_index_states")
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx_error)
    }

    pub async fn request_rebuild(&self, space_id: Uuid) -> Result<SpaceLinkIndexState> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        sqlx::query(
            "INSERT INTO space_link_index_states (space_id) VALUES ($1) \
             ON CONFLICT (space_id) DO NOTHING",
        )
        .bind(space_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let current = sqlx::query_as::<_, RebuildRequestStateRow>(
            "SELECT space_id, desired_generation, applied_generation, status, last_indexed_at, \
                    rebuild_base_generation \
             FROM space_link_index_states WHERE space_id = $1 FOR UPDATE",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if current.is_rebuilding() {
            tx.commit().await.map_err(map_sqlx_error)?;
            return current.into_public();
        }

        let resume_rebuild = current.rebuild_base_generation.is_some();

        let row = sqlx::query_as::<_, LinkIndexStateRow>(
            "UPDATE space_link_index_states \
             SET rebuild_requested = NOT $2, \
                 status = CASE \
                    WHEN $2 THEN 'rebuilding' \
                    WHEN space_link_index_states.status = 'uninitialized' THEN 'rebuilding' \
                    WHEN space_link_index_states.status = 'running' \
                        THEN space_link_index_states.status \
                    ELSE 'queued' \
                 END, \
                 run_after = now(), \
                 retry_count = 0, \
                 last_error = NULL, \
                 updated_at = now() \
             WHERE space_id = $1 \
             RETURNING space_id, desired_generation, applied_generation, status, last_indexed_at",
        )
        .bind(space_id)
        .bind(resume_rebuild)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)?;
        row.into_public()
    }

    pub async fn claim_next(
        &self,
        lease: Duration,
        parser_version: i32,
    ) -> Result<Option<LinkIndexClaim>> {
        let token = Uuid::new_v4();
        let lease_seconds = duration_seconds(lease)?;
        let row = sqlx::query_as::<_, ClaimedLinkIndexRow>(
            "WITH candidate AS ( \
                SELECT space_id \
                FROM space_link_index_states \
                WHERE run_after <= now() \
                  AND (claim_until IS NULL OR claim_until <= now()) \
                  AND parser_version <= $3 \
                  AND (rebuild_requested \
                       OR (status <> 'uninitialized' \
                           AND (status <> 'ready' OR applied_generation < desired_generation \
                                OR parser_version < $3))) \
                ORDER BY run_after, updated_at, space_id \
                FOR UPDATE SKIP LOCKED \
                LIMIT 1 \
             ) \
             UPDATE space_link_index_states state \
             SET claim_token = $1, \
                 claim_until = now() + make_interval(secs => $2), \
                 status = CASE \
                    WHEN state.status = 'rebuilding' \
                         OR state.rebuild_requested \
                         OR state.rebuild_base_generation IS NOT NULL \
                         OR state.parser_version < $3 \
                        THEN 'rebuilding' \
                    ELSE 'running' \
                 END, \
                 updated_at = now() \
             FROM candidate \
             WHERE state.space_id = candidate.space_id \
             RETURNING state.space_id, state.desired_generation, state.applied_generation, \
                       state.status, state.rebuild_requested, state.rebuild_base_generation, \
                       state.rebuild_after_node_id, state.parser_version, state.retry_count",
        )
        .bind(token)
        .bind(lease_seconds)
        .bind(parser_version)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(|row| row.into_claim(token)).transpose()
    }

    pub async fn events_after(
        &self,
        claim: &LinkIndexClaim,
        limit: i64,
    ) -> Result<LinkIndexEventBatch> {
        let rows = sqlx::query_as::<_, FileChangeEventRow>(
            "SELECT id, created_at, space_id, node_id, actor_account_id, op_type, metadata, \
                    link_index_generation \
             FROM file_change_events \
             WHERE space_id = $1 \
               AND link_index_generation > $2 \
               AND link_index_generation <= $3 \
             ORDER BY link_index_generation ASC LIMIT $4",
        )
        .bind(claim.space_id)
        .bind(claim.applied_generation)
        .bind(claim.desired_generation)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let cursor_valid = generations_are_contiguous(&rows, claim.applied_generation);

        Ok(LinkIndexEventBatch {
            events: rows
                .into_iter()
                .map(FileChangeEventRow::into_queued)
                .collect(),
            cursor_valid,
        })
    }

    pub async fn begin_rebuild(
        &self,
        claim: &LinkIndexClaim,
        parser_version: i32,
        lease: Duration,
    ) -> Result<i64> {
        let lease_seconds = duration_seconds(lease)?;
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        assert_claim(&mut tx, claim).await?;
        let base_generation: i64 = sqlx::query_scalar(
            "UPDATE space_link_index_states \
             SET status = 'rebuilding', \
                 rebuild_requested = false, \
                 rebuild_base_generation = desired_generation, \
                 rebuild_after_node_id = NULL, \
                 parser_version = $3, \
                 claim_until = now() + make_interval(secs => $4), \
                 last_error = NULL, \
                 updated_at = now() \
             WHERE space_id = $1 AND claim_token = $2 \
             RETURNING rebuild_base_generation",
        )
        .bind(claim.space_id)
        .bind(claim.token)
        .bind(parser_version)
        .bind(lease_seconds)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(base_generation)
    }

    pub async fn rebuild_source_ids(
        &self,
        space_id: Uuid,
        after_node_id: Option<Uuid>,
        limit: i64,
    ) -> Result<(Vec<Uuid>, bool)> {
        let rows = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM nodes \
             WHERE space_id = $1 \
               AND kind = 'text' \
               AND parent_id IS NOT NULL \
               AND deleted_at IS NULL \
               AND ($2::uuid IS NULL OR id > $2) \
             ORDER BY id LIMIT $3",
        )
        .bind(space_id)
        .bind(after_node_id)
        .bind(limit + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        let has_more = rows.len()
            > usize::try_from(limit)
                .map_err(|_| Error::internal("invalid link index batch limit"))?;
        let mut rows = rows;
        if has_more {
            rows.pop();
        }
        Ok((rows, has_more))
    }

    pub async fn rewrite_sources(
        &self,
        claim: &LinkIndexClaim,
        sources: &[SourceLinkSet],
        lease: Duration,
    ) -> Result<()> {
        if sources.is_empty() {
            return Ok(());
        }
        let lease_seconds = duration_seconds(lease)?;
        let source_ids = sources
            .iter()
            .map(|source| source.source_node_id)
            .collect::<Vec<_>>();

        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        renew_claim(&mut tx, claim, lease_seconds).await?;
        sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1 AND source_node_id = ANY($2)")
            .bind(claim.space_id)
            .bind(source_ids)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)?;

        let references = sources
            .iter()
            .flat_map(|source| {
                source
                    .references
                    .iter()
                    .map(move |reference| (source.source_node_id, reference))
            })
            .collect::<Vec<_>>();
        for chunk in references.chunks(LINK_INSERT_CHUNK_SIZE) {
            let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
            renew_claim(&mut tx, claim, lease_seconds).await?;
            insert_references(&mut tx, claim.space_id, chunk).await?;
            tx.commit().await.map_err(map_sqlx_error)?;
        }
        Ok(())
    }

    pub async fn commit_rebuild_batch(
        &self,
        claim: &LinkIndexClaim,
        after_node_id: Uuid,
        has_more: bool,
        base_generation: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        assert_claim(&mut tx, claim).await?;
        if has_more {
            let updated = sqlx::query(
                "UPDATE space_link_index_states \
                 SET rebuild_after_node_id = $3, \
                     claim_token = NULL, \
                     claim_until = NULL, \
                     run_after = now(), \
                     updated_at = now() \
                 WHERE space_id = $1 AND claim_token = $2 AND status = 'rebuilding'",
            )
            .bind(claim.space_id)
            .bind(claim.token)
            .bind(after_node_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
            if updated.rows_affected() != 1 {
                return Err(Error::conflict("link index claim was lost"));
            }
        } else {
            remove_deleted_sources(&mut tx, claim.space_id).await?;
            finish_rebuild_update(&mut tx, claim, base_generation).await?;
        }
        tx.commit().await.map_err(map_sqlx_error)
    }

    pub async fn finish_rebuild(&self, claim: &LinkIndexClaim, base_generation: i64) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        assert_claim(&mut tx, claim).await?;
        remove_deleted_sources(&mut tx, claim.space_id).await?;
        finish_rebuild_update(&mut tx, claim, base_generation).await?;
        tx.commit().await.map_err(map_sqlx_error)
    }

    pub async fn commit_incremental(
        &self,
        claim: &LinkIndexClaim,
        rebind_targets: &[(String, Uuid)],
        cleanup_deleted: bool,
        applied_generation: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        assert_claim(&mut tx, claim).await?;
        if cleanup_deleted {
            remove_deleted_sources(&mut tx, claim.space_id).await?;
        }
        rebind_targets_by_path(&mut tx, claim.space_id, rebind_targets).await?;
        let updated = sqlx::query(
            "UPDATE space_link_index_states \
             SET applied_generation = $3, \
                 status = CASE \
                    WHEN rebuild_requested THEN 'queued' \
                    WHEN desired_generation > $3 THEN 'queued' \
                    ELSE 'ready' \
                 END, \
                 claim_token = NULL, \
                 claim_until = NULL, \
                 retry_count = 0, \
                 run_after = now(), \
                 last_error = NULL, \
                 last_indexed_at = now(), \
                 updated_at = now() \
             WHERE space_id = $1 AND claim_token = $2",
        )
        .bind(claim.space_id)
        .bind(claim.token)
        .bind(applied_generation)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if updated.rows_affected() != 1 {
            return Err(Error::conflict("link index claim was lost"));
        }
        tx.commit().await.map_err(map_sqlx_error)
    }

    pub async fn request_claim_rebuild(&self, claim: &LinkIndexClaim) -> Result<()> {
        let updated = sqlx::query(
            "UPDATE space_link_index_states \
             SET rebuild_requested = true, status = 'queued', \
                 claim_token = NULL, claim_until = NULL, run_after = now(), updated_at = now() \
             WHERE space_id = $1 AND claim_token = $2",
        )
        .bind(claim.space_id)
        .bind(claim.token)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        if updated.rows_affected() != 1 {
            return Err(Error::conflict("link index claim was lost"));
        }
        Ok(())
    }

    pub async fn fail_claim(&self, claim: &LinkIndexClaim, error: &str) -> Result<()> {
        let exponent = u32::try_from(claim.retry_count.clamp(0, 6))
            .map_err(|_| Error::internal("invalid link index retry count"))?;
        let retry_after = 5_i32.saturating_mul(2_i32.saturating_pow(exponent));
        sqlx::query(
            "UPDATE space_link_index_states \
             SET status = 'failed', \
                 claim_token = NULL, \
                 claim_until = NULL, \
                 retry_count = retry_count + 1, \
                 run_after = now() + make_interval(secs => $3), \
                 last_error = left($4, 1000), \
                 updated_at = now() \
             WHERE space_id = $1 AND claim_token = $2",
        )
        .bind(claim.space_id)
        .bind(claim.token)
        .bind(retry_after)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}

async fn finish_rebuild_update(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    claim: &LinkIndexClaim,
    base_generation: i64,
) -> Result<()> {
    let updated = sqlx::query(
        "UPDATE space_link_index_states \
         SET applied_generation = $3, \
             status = CASE \
                WHEN rebuild_requested OR desired_generation > $3 THEN 'queued' \
                ELSE 'ready' \
             END, \
             rebuild_base_generation = NULL, \
             rebuild_after_node_id = NULL, \
             claim_token = NULL, \
             claim_until = NULL, \
             retry_count = 0, \
             run_after = now(), \
             last_error = NULL, \
             last_indexed_at = now(), \
             updated_at = now() \
         WHERE space_id = $1 AND claim_token = $2",
    )
    .bind(claim.space_id)
    .bind(claim.token)
    .bind(base_generation)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::conflict("link index claim was lost"));
    }
    Ok(())
}

async fn assert_claim(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    claim: &LinkIndexClaim,
) -> Result<()> {
    let claimed_token = sqlx::query_scalar::<_, Uuid>(
        "SELECT claim_token FROM space_link_index_states \
         WHERE space_id = $1 AND claim_token = $2 AND claim_until > now() \
         FOR UPDATE",
    )
    .bind(claim.space_id)
    .bind(claim.token)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;
    if claimed_token.is_none() {
        return Err(Error::conflict("link index claim was lost"));
    }
    Ok(())
}

async fn renew_claim(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    claim: &LinkIndexClaim,
    lease_seconds: i32,
) -> Result<()> {
    let updated = sqlx::query(
        "UPDATE space_link_index_states \
         SET claim_until = now() + make_interval(secs => $3), updated_at = now() \
         WHERE space_id = $1 AND claim_token = $2 AND claim_until > now()",
    )
    .bind(claim.space_id)
    .bind(claim.token)
    .bind(lease_seconds)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::conflict("link index claim was lost"));
    }
    Ok(())
}

async fn insert_references(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    space_id: Uuid,
    references: &[(Uuid, &NewLinkReference)],
) -> Result<()> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO node_link_refs (space_id, source_node_id, target_node_id, \
         reference_kind, raw_href, normalized_target_path, occurrence_count) ",
    );
    builder.push_values(references, |mut row, (source_node_id, reference)| {
        row.push_bind(space_id)
            .push_bind(source_node_id)
            .push_bind(reference.target_node_id)
            .push_bind(reference.kind.as_str())
            .push_bind(&reference.raw_href)
            .push_bind(&reference.normalized_target_path)
            .push_bind(reference.occurrence_count);
    });
    builder
        .build()
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx_error)?;
    Ok(())
}

async fn remove_deleted_sources(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    space_id: Uuid,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM node_link_refs reference \
         WHERE reference.space_id = $1 \
           AND NOT EXISTS ( \
                SELECT 1 FROM nodes source \
                WHERE source.id = reference.source_node_id \
                  AND source.space_id = reference.space_id \
                  AND source.deleted_at IS NULL \
           )",
    )
    .bind(space_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

async fn rebind_targets_by_path(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    space_id: Uuid,
    targets: &[(String, Uuid)],
) -> Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    let paths = targets
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    let node_ids = targets
        .iter()
        .map(|(_, node_id)| *node_id)
        .collect::<Vec<_>>();
    sqlx::query(
        "UPDATE node_link_refs reference \
         SET target_node_id = resolved.node_id \
         FROM unnest($2::text[], $3::uuid[]) AS resolved(path, node_id) \
         WHERE reference.space_id = $1 \
           AND md5(reference.normalized_target_path) = md5(resolved.path) \
           AND reference.normalized_target_path = resolved.path \
           AND ( \
                reference.target_node_id IS NULL \
                OR EXISTS ( \
                    SELECT 1 FROM nodes old_target \
                    WHERE old_target.id = reference.target_node_id \
                      AND old_target.deleted_at IS NOT NULL \
                ) \
           )",
    )
    .bind(space_id)
    .bind(paths)
    .bind(node_ids)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

fn duration_seconds(duration: Duration) -> Result<i32> {
    i32::try_from(duration.as_secs())
        .map_err(|_| Error::internal("link index lease duration exceeds i32"))
}

#[derive(Debug, FromRow)]
struct LinkIndexStateRow {
    space_id: Uuid,
    desired_generation: i64,
    applied_generation: i64,
    status: String,
    last_indexed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
struct RebuildRequestStateRow {
    space_id: Uuid,
    desired_generation: i64,
    applied_generation: i64,
    status: String,
    last_indexed_at: Option<DateTime<Utc>>,
    rebuild_base_generation: Option<i64>,
}

impl RebuildRequestStateRow {
    fn is_rebuilding(&self) -> bool {
        self.status == "rebuilding"
    }

    fn into_public(self) -> Result<SpaceLinkIndexState> {
        LinkIndexStateRow {
            space_id: self.space_id,
            desired_generation: self.desired_generation,
            applied_generation: self.applied_generation,
            status: self.status,
            last_indexed_at: self.last_indexed_at,
        }
        .into_public()
    }
}

impl LinkIndexStateRow {
    fn into_public(self) -> Result<SpaceLinkIndexState> {
        let status = LinkIndexStatus::parse(&self.status).ok_or_else(|| {
            Error::internal(format!("unknown link index status: {}", self.status))
        })?;
        Ok(SpaceLinkIndexState {
            space_id: self.space_id,
            desired_generation: self.desired_generation,
            applied_generation: self.applied_generation,
            status,
            last_indexed_at: self.last_indexed_at,
        })
    }
}

#[derive(Debug, FromRow)]
struct ClaimedLinkIndexRow {
    space_id: Uuid,
    desired_generation: i64,
    applied_generation: i64,
    status: String,
    rebuild_requested: bool,
    rebuild_base_generation: Option<i64>,
    rebuild_after_node_id: Option<Uuid>,
    parser_version: i32,
    retry_count: i32,
}

impl ClaimedLinkIndexRow {
    fn into_claim(self, token: Uuid) -> Result<LinkIndexClaim> {
        let status = LinkIndexStatus::parse(&self.status).ok_or_else(|| {
            Error::internal(format!("unknown link index status: {}", self.status))
        })?;
        Ok(LinkIndexClaim {
            space_id: self.space_id,
            token,
            desired_generation: self.desired_generation,
            applied_generation: self.applied_generation,
            status,
            rebuild_requested: self.rebuild_requested,
            rebuild_base_generation: self.rebuild_base_generation,
            rebuild_after_node_id: self.rebuild_after_node_id,
            parser_version: self.parser_version,
            retry_count: self.retry_count,
        })
    }
}

#[derive(Debug, FromRow)]
struct FileChangeEventRow {
    id: i64,
    created_at: DateTime<Utc>,
    space_id: Uuid,
    node_id: Option<Uuid>,
    actor_account_id: Option<Uuid>,
    op_type: String,
    metadata: Value,
    link_index_generation: i64,
}

impl FileChangeEventRow {
    fn into_queued(self) -> QueuedLinkIndexEvent {
        QueuedLinkIndexEvent {
            generation: self.link_index_generation,
            event: FileChangeEvent {
                id: self.id,
                created_at: self.created_at,
                space_id: self.space_id,
                node_id: self.node_id,
                actor_account_id: self.actor_account_id,
                op_type: self.op_type,
                metadata: self.metadata,
            },
        }
    }
}

fn generations_are_contiguous(rows: &[FileChangeEventRow], applied_generation: i64) -> bool {
    let Some(mut expected) = applied_generation.checked_add(1) else {
        return false;
    };
    for row in rows {
        if row.link_index_generation != expected {
            return false;
        }
        let Some(next) = expected.checked_add(1) else {
            return false;
        };
        expected = next;
    }
    true
}
