use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_model::LinkReferenceKind;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

const CLAIM_LEASE: &str = "2 minutes";
const RETRY_DELAY: &str = "30 seconds";

#[derive(Debug, Clone)]
pub struct LinkIndexRepo {
    pool: PgPool,
}

impl LinkIndexRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn request_source(&self, space_id: Uuid, source_node_id: Uuid) -> Result<bool> {
        sqlx::query_scalar("SELECT enqueue_node_link_source($1, $2)")
            .bind(space_id)
            .bind(source_node_id)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx_error)
    }

    pub async fn request_space(&self, space_id: Uuid) -> Result<bool> {
        sqlx::query_scalar("SELECT enqueue_node_link_space($1)")
            .bind(space_id)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx_error)
    }

    pub async fn claim_space(&self) -> Result<Option<SpaceLinkClaim>> {
        let token = Uuid::new_v4();
        sqlx::query_as(
            "WITH candidate AS ( \
                SELECT state.space_id, state.requested_version \
                FROM node_link_space_reindex_states state \
                JOIN spaces space ON space.id = state.space_id AND space.deleted_at IS NULL \
                WHERE state.requested_version > state.applied_version \
                  AND state.run_after <= now() \
                  AND (state.claim_until IS NULL OR state.claim_until <= now()) \
                ORDER BY state.run_after, state.space_id \
                LIMIT 1 FOR UPDATE OF state SKIP LOCKED \
             ) \
             UPDATE node_link_space_reindex_states state \
             SET claim_token = $1, claim_until = now() + $2::interval \
             FROM candidate \
             WHERE state.space_id = candidate.space_id \
             RETURNING state.space_id, candidate.requested_version, state.claim_token",
        )
        .bind(token)
        .bind(CLAIM_LEASE)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn expand_space(&self, claim: &SpaceLinkClaim) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let owns_claim = sqlx::query_scalar(
            "SELECT true FROM node_link_space_reindex_states \
             WHERE space_id = $1 AND claim_token = $2 \
             FOR UPDATE",
        )
        .bind(claim.space_id)
        .bind(claim.claim_token)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .unwrap_or(false);
        if !owns_claim {
            tx.rollback().await.map_err(map_sqlx_error)?;
            return Ok(false);
        }

        sqlx::query(
            "DELETE FROM node_link_refs refs \
             WHERE refs.space_id = $1 \
               AND NOT EXISTS ( \
                   SELECT 1 FROM nodes node \
                   WHERE node.space_id = refs.space_id \
                     AND node.id = refs.source_node_id \
                     AND node.kind = 'text' \
                     AND node.deleted_at IS NULL \
               )",
        )
        .bind(claim.space_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        sqlx::query(
            "DELETE FROM node_link_source_states state \
             WHERE state.space_id = $1 \
               AND NOT EXISTS ( \
                   SELECT 1 FROM nodes node \
                   WHERE node.space_id = state.space_id \
                     AND node.id = state.source_node_id \
                     AND node.kind = 'text' \
                     AND node.deleted_at IS NULL \
               )",
        )
        .bind(claim.space_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        sqlx::query(
            "INSERT INTO node_link_source_states (space_id, source_node_id) \
             SELECT space_id, id FROM nodes \
             WHERE space_id = $1 AND kind = 'text' AND deleted_at IS NULL \
             ON CONFLICT (space_id, source_node_id) DO UPDATE \
             SET requested_version = node_link_source_states.requested_version + 1, \
                 run_after = now(), last_error = NULL",
        )
        .bind(claim.space_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        sqlx::query(
            "UPDATE node_link_space_reindex_states \
             SET applied_version = $3, claim_token = NULL, claim_until = NULL, \
                 last_error = NULL \
             WHERE space_id = $1 AND claim_token = $2",
        )
        .bind(claim.space_id)
        .bind(claim.claim_token)
        .bind(claim.requested_version)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(true)
    }

    pub async fn fail_space(&self, claim: &SpaceLinkClaim, error: &str) -> Result<()> {
        sqlx::query(
            "UPDATE node_link_space_reindex_states \
             SET claim_token = NULL, claim_until = NULL, \
                 run_after = now() + $3::interval, last_error = $4 \
             WHERE space_id = $1 AND claim_token = $2",
        )
        .bind(claim.space_id)
        .bind(claim.claim_token)
        .bind(RETRY_DELAY)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn claim_source(&self) -> Result<Option<SourceLinkClaim>> {
        let token = Uuid::new_v4();
        sqlx::query_as(
            "WITH candidate AS ( \
                SELECT state.space_id, state.source_node_id, state.requested_version \
                FROM node_link_source_states state \
                JOIN spaces space ON space.id = state.space_id AND space.deleted_at IS NULL \
                WHERE state.requested_version > state.applied_version \
                  AND state.run_after <= now() \
                  AND (state.claim_until IS NULL OR state.claim_until <= now()) \
                ORDER BY state.run_after, state.source_node_id \
                LIMIT 1 FOR UPDATE OF state SKIP LOCKED \
             ) \
             UPDATE node_link_source_states state \
             SET claim_token = $1, claim_until = now() + $2::interval \
             FROM candidate \
             WHERE state.space_id = candidate.space_id \
               AND state.source_node_id = candidate.source_node_id \
             RETURNING state.space_id, state.source_node_id, \
                       candidate.requested_version, state.claim_token",
        )
        .bind(token)
        .bind(CLAIM_LEASE)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn complete_source(
        &self,
        claim: &SourceLinkClaim,
        references: &[StoredLinkReference],
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let owns_claim = sqlx::query_scalar(
            "SELECT true FROM node_link_source_states \
             WHERE space_id = $1 AND source_node_id = $2 AND claim_token = $3 \
             FOR UPDATE",
        )
        .bind(claim.space_id)
        .bind(claim.source_node_id)
        .bind(claim.claim_token)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .unwrap_or(false);
        if !owns_claim {
            tx.rollback().await.map_err(map_sqlx_error)?;
            return Ok(false);
        }

        sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1 AND source_node_id = $2")
            .bind(claim.space_id)
            .bind(claim.source_node_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

        if !references.is_empty() {
            let kinds = references
                .iter()
                .map(|reference| reference.kind.as_str())
                .collect::<Vec<_>>();
            let paths = references
                .iter()
                .map(|reference| reference.target_path.as_str())
                .collect::<Vec<_>>();
            let target_ids = references
                .iter()
                .map(|reference| reference.target_node_id)
                .collect::<Vec<_>>();
            let counts = references
                .iter()
                .map(|reference| reference.occurrence_count)
                .collect::<Vec<_>>();
            sqlx::query(
                "INSERT INTO node_link_refs ( \
                    space_id, source_node_id, reference_kind, target_path, \
                    target_node_id, occurrence_count \
                 ) \
                 SELECT $1, $2, input.kind, input.path, input.target_id, input.count \
                 FROM unnest($3::text[], $4::text[], $5::uuid[], $6::integer[]) \
                      AS input(kind, path, target_id, count)",
            )
            .bind(claim.space_id)
            .bind(claim.source_node_id)
            .bind(kinds)
            .bind(paths)
            .bind(target_ids)
            .bind(counts)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        }

        sqlx::query(
            "UPDATE node_link_source_states \
             SET applied_version = $4, claim_token = NULL, claim_until = NULL, \
                 last_error = NULL, last_synced_at = now() \
             WHERE space_id = $1 AND source_node_id = $2 AND claim_token = $3",
        )
        .bind(claim.space_id)
        .bind(claim.source_node_id)
        .bind(claim.claim_token)
        .bind(claim.requested_version)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(true)
    }

    pub async fn discard_source(&self, claim: &SourceLinkClaim) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let deleted = sqlx::query(
            "DELETE FROM node_link_source_states \
             WHERE space_id = $1 AND source_node_id = $2 AND claim_token = $3",
        )
        .bind(claim.space_id)
        .bind(claim.source_node_id)
        .bind(claim.claim_token)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected();
        if deleted > 0 {
            sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1 AND source_node_id = $2")
                .bind(claim.space_id)
                .bind(claim.source_node_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(deleted > 0)
    }

    pub async fn fail_source(&self, claim: &SourceLinkClaim, error: &str) -> Result<()> {
        sqlx::query(
            "UPDATE node_link_source_states \
             SET claim_token = NULL, claim_until = NULL, \
                 run_after = now() + $4::interval, last_error = $5 \
             WHERE space_id = $1 AND source_node_id = $2 AND claim_token = $3",
        )
        .bind(claim.space_id)
        .bind(claim.source_node_id)
        .bind(claim.claim_token)
        .bind(RETRY_DELAY)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn source_state(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
    ) -> Result<Option<LinkSourceState>> {
        sqlx::query_as(
            "SELECT requested_version, applied_version, claim_until, last_error, last_synced_at \
             FROM node_link_source_states \
             WHERE space_id = $1 AND source_node_id = $2",
        )
        .bind(space_id)
        .bind(source_node_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn outgoing(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
    ) -> Result<Vec<StoredLinkReference>> {
        let rows = sqlx::query_as::<_, StoredLinkReferenceRow>(
            "SELECT target_node_id, target_path, reference_kind, occurrence_count \
             FROM node_link_refs \
             WHERE space_id = $1 AND source_node_id = $2 \
             ORDER BY target_path, reference_kind",
        )
        .bind(space_id)
        .bind(source_node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.into_iter()
            .map(StoredLinkReference::try_from)
            .collect()
    }

    pub async fn incoming(
        &self,
        space_id: Uuid,
        target_node_id: Uuid,
    ) -> Result<Vec<IncomingLinkReference>> {
        let rows = sqlx::query_as::<_, IncomingLinkReferenceRow>(
            "SELECT source_node_id, reference_kind, occurrence_count \
             FROM node_link_refs \
             WHERE space_id = $1 AND target_node_id = $2 \
             ORDER BY source_node_id, reference_kind",
        )
        .bind(space_id)
        .bind(target_node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.into_iter()
            .map(IncomingLinkReference::try_from)
            .collect()
    }

    pub async fn space_status(&self, space_id: Uuid) -> Result<SpaceLinkStatus> {
        sqlx::query_as(
            "SELECT \
                count(*) FILTER ( \
                    WHERE node.id IS NOT NULL \
                      AND (source.source_node_id IS NULL \
                       OR source.requested_version > source.applied_version \
                      ) \
                )::bigint AS pending_documents, \
                count(*) FILTER ( \
                    WHERE node.id IS NOT NULL \
                      AND source.requested_version > source.applied_version \
                      AND source.last_error IS NOT NULL \
                )::bigint AS retrying_documents, \
                count(*) FILTER ( \
                    WHERE node.id IS NOT NULL \
                      AND source.requested_version > source.applied_version \
                      AND source.claim_until > now() \
                )::bigint AS syncing_documents, \
                max(source.last_synced_at) AS last_synced_at, \
                COALESCE(space_state.requested_version > space_state.applied_version, false) \
                    AS space_pending, \
                COALESCE(space_state.claim_until > now(), false) AS space_syncing, \
                space_state.last_error AS space_error \
             FROM spaces space \
             LEFT JOIN nodes node \
               ON node.space_id = space.id \
              AND node.kind = 'text' \
              AND node.deleted_at IS NULL \
             LEFT JOIN node_link_source_states source \
               ON source.space_id = space.id AND source.source_node_id = node.id \
             LEFT JOIN node_link_space_reindex_states space_state \
               ON space_state.space_id = space.id \
             WHERE space.id = $1 AND space.deleted_at IS NULL \
             GROUP BY space_state.requested_version, space_state.applied_version, \
                      space_state.claim_until, space_state.last_error",
        )
        .bind(space_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
        .map(|status| status.unwrap_or_default())
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct SourceLinkClaim {
    pub space_id: Uuid,
    pub source_node_id: Uuid,
    pub requested_version: i64,
    pub claim_token: Uuid,
}

#[derive(Debug, Clone, FromRow)]
pub struct SpaceLinkClaim {
    pub space_id: Uuid,
    pub requested_version: i64,
    pub claim_token: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLinkReference {
    pub target_node_id: Option<Uuid>,
    pub target_path: String,
    pub kind: LinkReferenceKind,
    pub occurrence_count: i32,
}

#[derive(Debug, FromRow)]
struct StoredLinkReferenceRow {
    target_node_id: Option<Uuid>,
    target_path: String,
    reference_kind: String,
    occurrence_count: i32,
}

impl TryFrom<StoredLinkReferenceRow> for StoredLinkReference {
    type Error = notegate_core::Error;

    fn try_from(row: StoredLinkReferenceRow) -> Result<Self> {
        let kind = LinkReferenceKind::parse(&row.reference_kind).ok_or_else(|| {
            notegate_core::Error::internal(format!(
                "unknown link reference kind: {}",
                row.reference_kind
            ))
        })?;
        Ok(Self {
            target_node_id: row.target_node_id,
            target_path: row.target_path,
            kind,
            occurrence_count: row.occurrence_count,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingLinkReference {
    pub source_node_id: Uuid,
    pub kind: LinkReferenceKind,
    pub occurrence_count: i32,
}

#[derive(Debug, FromRow)]
struct IncomingLinkReferenceRow {
    source_node_id: Uuid,
    reference_kind: String,
    occurrence_count: i32,
}

impl TryFrom<IncomingLinkReferenceRow> for IncomingLinkReference {
    type Error = notegate_core::Error;

    fn try_from(row: IncomingLinkReferenceRow) -> Result<Self> {
        let kind = LinkReferenceKind::parse(&row.reference_kind).ok_or_else(|| {
            notegate_core::Error::internal(format!(
                "unknown link reference kind: {}",
                row.reference_kind
            ))
        })?;
        Ok(Self {
            source_node_id: row.source_node_id,
            kind,
            occurrence_count: row.occurrence_count,
        })
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct LinkSourceState {
    pub requested_version: i64,
    pub applied_version: i64,
    pub claim_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, FromRow)]
pub struct SpaceLinkStatus {
    pub pending_documents: i64,
    pub retrying_documents: i64,
    pub syncing_documents: i64,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub space_pending: bool,
    pub space_syncing: bool,
    pub space_error: Option<String>,
}
