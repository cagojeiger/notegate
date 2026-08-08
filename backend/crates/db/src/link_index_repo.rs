use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_model::LinkReferenceKind;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{ReconciliationClaim, ReconciliationRepo, map_sqlx_error};

const PROJECTION_QUEUE: &str = "projection";
const SOURCE_WORK_KIND: &str = "node_link_source";
const SPACE_WORK_KIND: &str = "node_link_space";
const CLAIM_LEASE: std::time::Duration = std::time::Duration::from_secs(120);
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct LinkIndexRepo {
    pool: PgPool,
    work: ReconciliationRepo,
}

impl LinkIndexRepo {
    pub fn new(pool: PgPool) -> Self {
        Self {
            work: ReconciliationRepo::new(pool.clone()),
            pool,
        }
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

    pub async fn claim_work(&self) -> Result<Option<ReconciliationClaim>> {
        self.work.claim_one(PROJECTION_QUEUE, CLAIM_LEASE).await
    }

    pub async fn expand_space(&self, claim: &ReconciliationClaim) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        if !ReconciliationRepo::owns_claim_in(&mut tx, claim).await? {
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
            "DELETE FROM reconciliation_work_items work \
             WHERE work.queue_name = 'projection' \
               AND work.work_kind = 'node_link_source' \
               AND work.space_id = $1 \
               AND NOT EXISTS ( \
                   SELECT 1 FROM nodes node \
                   WHERE node.space_id = work.space_id \
                     AND node.id = work.target_id \
                     AND node.kind = 'text' \
                     AND node.deleted_at IS NULL \
               )",
        )
        .bind(claim.space_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        sqlx::query(
            "SELECT enqueue_reconciliation_work( \
                 'projection', 'node_link_source', space_id, id \
             ) \
             FROM nodes \
             WHERE space_id = $1 AND kind = 'text' AND deleted_at IS NULL",
        )
        .bind(claim.space_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if !ReconciliationRepo::complete_in(&mut tx, claim).await? {
            tx.rollback().await.map_err(map_sqlx_error)?;
            return Ok(false);
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(true)
    }

    pub fn is_space_work(claim: &ReconciliationClaim) -> bool {
        claim.work_kind == SPACE_WORK_KIND
    }

    pub fn is_source_work(claim: &ReconciliationClaim) -> bool {
        claim.work_kind == SOURCE_WORK_KIND
    }

    pub async fn fail_work(&self, claim: &ReconciliationClaim, error: &str) -> Result<()> {
        self.work.fail(claim, RETRY_DELAY, error).await?;
        Ok(())
    }

    pub async fn complete_source(
        &self,
        claim: &ReconciliationClaim,
        references: &[StoredLinkReference],
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        if !ReconciliationRepo::owns_claim_in(&mut tx, claim).await? {
            tx.rollback().await.map_err(map_sqlx_error)?;
            return Ok(false);
        }

        sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1 AND source_node_id = $2")
            .bind(claim.space_id)
            .bind(claim.target_id)
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
            .bind(claim.target_id)
            .bind(kinds)
            .bind(paths)
            .bind(target_ids)
            .bind(counts)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        }

        if !ReconciliationRepo::complete_in(&mut tx, claim).await? {
            tx.rollback().await.map_err(map_sqlx_error)?;
            return Ok(false);
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(true)
    }

    pub async fn discard_source(&self, claim: &ReconciliationClaim) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let deleted = ReconciliationRepo::delete_in(&mut tx, claim).await?;
        if deleted {
            sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1 AND source_node_id = $2")
                .bind(claim.space_id)
                .bind(claim.target_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(deleted)
    }

    pub async fn source_state(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
    ) -> Result<Option<LinkSourceState>> {
        sqlx::query_as(
            "SELECT requested_generation AS requested_version, \
                    applied_generation AS applied_version, \
                    lease_until AS claim_until, last_error, \
                    last_completed_at AS last_synced_at \
             FROM reconciliation_work_items \
             WHERE queue_name = 'projection' AND work_kind = 'node_link_source' \
               AND space_id = $1 AND target_id = $2",
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
                      AND (source.target_id IS NULL \
                       OR source.requested_generation > source.applied_generation \
                      ) \
                )::bigint AS pending_documents, \
                count(*) FILTER ( \
                    WHERE node.id IS NOT NULL \
                      AND source.requested_generation > source.applied_generation \
                      AND source.last_error IS NOT NULL \
                )::bigint AS retrying_documents, \
                count(*) FILTER ( \
                    WHERE node.id IS NOT NULL \
                      AND source.requested_generation > source.applied_generation \
                      AND source.lease_until > now() \
                )::bigint AS syncing_documents, \
                max(source.last_completed_at) AS last_synced_at, \
                COALESCE( \
                    space_state.requested_generation > space_state.applied_generation, false \
                ) \
                    AS space_pending, \
                COALESCE(space_state.lease_until > now(), false) AS space_syncing, \
                space_state.last_error AS space_error \
             FROM spaces space \
             LEFT JOIN nodes node \
               ON node.space_id = space.id \
              AND node.kind = 'text' \
              AND node.deleted_at IS NULL \
             LEFT JOIN reconciliation_work_items source \
               ON source.queue_name = 'projection' \
              AND source.work_kind = 'node_link_source' \
              AND source.space_id = space.id AND source.target_id = node.id \
             LEFT JOIN reconciliation_work_items space_state \
               ON space_state.queue_name = 'projection' \
              AND space_state.work_kind = 'node_link_space' \
              AND space_state.space_id = space.id AND space_state.target_id = space.id \
             WHERE space.id = $1 AND space.deleted_at IS NULL \
             GROUP BY space_state.requested_generation, space_state.applied_generation, \
                      space_state.lease_until, space_state.last_error",
        )
        .bind(space_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
        .map(|status| status.unwrap_or_default())
    }
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
