use chrono::{DateTime, Utc};
use notegate_core::{Error, Result};
use notegate_jobs::{BACKGROUND_JOB_NOTIFY_CHANNEL, ClaimFence, JobQueue, JobSpec};
use notegate_model::{IncomingLinkCursor, LinkReferenceKind, OutgoingLinkCursor};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::map_sqlx_error;

pub const LINK_SOURCE_JOB_KIND: &str = "node_link_source";
pub const LINK_IMPACT_JOB_KIND: &str = "node_link_impact";
pub const LINK_SPACE_JOB_KIND: &str = "node_link_space";
pub const LINK_PARSER_VERSION: i32 = 1;

pub struct LinkSourceJob;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkSourcePayload {
    pub space_id: Uuid,
    pub source_node_id: Uuid,
}

impl JobSpec for LinkSourceJob {
    const KIND: &'static str = LINK_SOURCE_JOB_KIND;
    type Payload = LinkSourcePayload;
}

pub struct LinkImpactJob;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkImpactPayload {
    pub space_id: Uuid,
    pub changed_node_id: Uuid,
}

impl JobSpec for LinkImpactJob {
    const KIND: &'static str = LINK_IMPACT_JOB_KIND;
    type Payload = LinkImpactPayload;
}

pub struct LinkSpaceJob;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkSpacePayload {
    pub space_id: Uuid,
}

impl JobSpec for LinkSpaceJob {
    const KIND: &'static str = LINK_SPACE_JOB_KIND;
    type Payload = LinkSpacePayload;
}

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

    pub async fn expand_impact(
        &self,
        fence: &ClaimFence,
        space_id: Uuid,
        changed_node_id: Uuid,
    ) -> Result<LinkExpansion> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let live_space: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM spaces WHERE id = $1 AND deleted_at IS NULL)",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if !live_space {
            if !commit_if_claim_owned(tx, fence).await? {
                return Ok(LinkExpansion::ClaimLost);
            }
            return Ok(LinkExpansion::Deleted);
        }

        let changed_node_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM nodes WHERE space_id = $1 AND id = $2)",
        )
        .bind(space_id)
        .bind(changed_node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if !changed_node_exists {
            if !commit_if_claim_owned(tx, fence).await? {
                return Ok(LinkExpansion::ClaimLost);
            }
            return Ok(LinkExpansion::Deleted);
        }

        let inserted = sqlx::query_scalar::<_, i64>(
            "WITH RECURSIVE changed_nodes AS ( \
                 SELECT id, parent_id, name, kind, deleted_at, \
                        deleted_at IS NOT NULL AS include_deleted \
                 FROM nodes WHERE space_id = $1 AND id = $2 \
                 UNION ALL \
                 SELECT child.id, child.parent_id, child.name, child.kind, child.deleted_at, \
                        parent.include_deleted \
                 FROM nodes child \
                 JOIN changed_nodes parent ON child.parent_id = parent.id \
                 WHERE child.space_id = $1 \
                   AND (parent.include_deleted OR child.deleted_at IS NULL) \
             ), path_chain AS ( \
                 SELECT changed.id AS target_node_id, changed.id AS node_id, \
                        changed.parent_id, changed.name, 0 AS depth \
                 FROM changed_nodes changed WHERE changed.deleted_at IS NULL \
                 UNION ALL \
                 SELECT chain.target_node_id, ancestor.id, ancestor.parent_id, \
                        ancestor.name, chain.depth + 1 \
                 FROM path_chain chain \
                 JOIN nodes ancestor ON ancestor.id = chain.parent_id \
                 WHERE ancestor.space_id = $1 AND ancestor.deleted_at IS NULL \
             ), current_paths AS ( \
                 SELECT target_node_id, \
                        CASE WHEN max(depth) = 0 THEN '/' \
                             ELSE '/' || string_agg(name, '/' ORDER BY depth DESC) \
                                  FILTER (WHERE parent_id IS NOT NULL) \
                        END AS target_path \
                 FROM path_chain GROUP BY target_node_id \
             ), affected_sources AS ( \
                 SELECT id AS source_node_id FROM changed_nodes WHERE kind = 'text' \
                 UNION \
                 SELECT refs.source_node_id \
                 FROM node_link_refs refs \
                 JOIN changed_nodes target ON target.id = refs.target_node_id \
                 WHERE refs.space_id = $1 \
                 UNION \
                 SELECT refs.source_node_id \
                 FROM node_link_refs refs \
                 JOIN current_paths path ON path.target_path = refs.target_path \
                 WHERE refs.space_id = $1 AND refs.target_node_id IS NULL \
             ), eligible_sources AS ( \
                 SELECT affected.source_node_id \
                 FROM affected_sources affected \
                 WHERE EXISTS ( \
                     SELECT 1 FROM nodes source \
                     WHERE source.space_id = $1 AND source.id = affected.source_node_id \
                       AND source.kind = 'text' AND source.deleted_at IS NULL \
                 ) OR EXISTS ( \
                     SELECT 1 FROM node_link_source_states state \
                     WHERE state.space_id = $1 \
                       AND state.source_node_id = affected.source_node_id \
                 ) OR EXISTS ( \
                     SELECT 1 FROM node_link_refs existing \
                     WHERE existing.space_id = $1 \
                       AND existing.source_node_id = affected.source_node_id \
                 ) \
             ), inserted AS ( \
                 INSERT INTO background_jobs ( \
                     job_kind, payload, available_at, max_attempts \
                 ) \
                 SELECT 'node_link_source', \
                        jsonb_build_object( \
                            'space_id', $1, 'source_node_id', source_node_id \
                        ), \
                        now(), 8 \
                 FROM eligible_sources \
                 ON CONFLICT DO NOTHING \
                 RETURNING 1 \
             ) \
             SELECT count(*) FROM inserted",
        )
        .bind(space_id)
        .bind(changed_node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        notify_source_jobs(&mut tx, inserted).await?;

        if !commit_if_claim_owned(tx, fence).await? {
            return Ok(LinkExpansion::ClaimLost);
        }
        Ok(LinkExpansion::Expanded)
    }

    pub async fn expand_space(&self, fence: &ClaimFence, space_id: Uuid) -> Result<LinkExpansion> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let live_space: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM spaces WHERE id = $1 AND deleted_at IS NULL)",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if !live_space {
            if !commit_if_claim_owned(tx, fence).await? {
                return Ok(LinkExpansion::ClaimLost);
            }
            return Ok(LinkExpansion::Deleted);
        }

        let inserted = sqlx::query_scalar::<_, i64>(
            "WITH source_candidates AS ( \
                 SELECT id AS source_node_id FROM nodes \
                 WHERE space_id = $1 AND kind = 'text' AND deleted_at IS NULL \
                 UNION \
                 SELECT source_node_id FROM node_link_source_states WHERE space_id = $1 \
                 UNION \
                 SELECT source_node_id FROM node_link_refs WHERE space_id = $1 \
             ), inserted AS ( \
                 INSERT INTO background_jobs ( \
                     job_kind, payload, available_at, max_attempts \
                 ) \
                 SELECT 'node_link_source', \
                        jsonb_build_object( \
                            'space_id', $1, 'source_node_id', source_node_id \
                        ), \
                        now(), 8 \
                 FROM source_candidates \
                 ON CONFLICT DO NOTHING \
                 RETURNING 1 \
             ) \
             SELECT count(*) FROM inserted",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        notify_source_jobs(&mut tx, inserted).await?;
        sqlx::query(
            "INSERT INTO node_link_space_states (space_id, expanded_at) \
             VALUES ($1, now()) \
             ON CONFLICT (space_id) DO UPDATE SET expanded_at = EXCLUDED.expanded_at",
        )
        .bind(space_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if !commit_if_claim_owned(tx, fence).await? {
            return Ok(LinkExpansion::ClaimLost);
        }
        Ok(LinkExpansion::Expanded)
    }

    pub async fn complete_source(
        &self,
        fence: &ClaimFence,
        space_id: Uuid,
        source_node_id: Uuid,
        expected_content_sha256: &str,
        expected_path: &str,
        references: &[NewLinkReference],
    ) -> Result<LinkSourceCommit> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let current = current_source_fingerprint(&mut tx, space_id, source_node_id).await?;
        let Some((current_sha256, current_path)) = current else {
            delete_source_projection(&mut tx, space_id, source_node_id).await?;
            if !commit_if_claim_owned(tx, fence).await? {
                return Ok(LinkSourceCommit::ClaimLost);
            }
            return Ok(LinkSourceCommit::Deleted);
        };
        if current_sha256 != expected_content_sha256 || current_path != expected_path {
            if !commit_if_claim_owned(tx, fence).await? {
                return Ok(LinkSourceCommit::ClaimLost);
            }
            return Ok(LinkSourceCommit::Stale);
        }

        sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1 AND source_node_id = $2")
            .bind(space_id)
            .bind(source_node_id)
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
            let counts = references
                .iter()
                .map(|reference| reference.occurrence_count)
                .collect::<Vec<_>>();
            sqlx::query(
                "WITH RECURSIVE requested AS ( \
                     SELECT input.kind, input.path, input.count, \
                            string_to_array(trim(both '/' from input.path), '/') AS segments \
                     FROM unnest($3::text[], $4::text[], $5::integer[]) \
                          AS input(kind, path, count) \
                 ), walk AS ( \
                     SELECT requested.kind, requested.path, requested.count, \
                            requested.segments, root.id AS node_id, 0 AS depth \
                     FROM requested \
                     JOIN nodes root ON root.space_id = $1 \
                        AND root.parent_id IS NULL AND root.deleted_at IS NULL \
                     UNION ALL \
                     SELECT walk.kind, walk.path, walk.count, walk.segments, \
                            child.id, walk.depth + 1 \
                     FROM walk \
                     JOIN nodes child ON child.space_id = $1 \
                        AND child.parent_id = walk.node_id \
                        AND child.name = walk.segments[walk.depth + 1] \
                        AND child.deleted_at IS NULL \
                     WHERE walk.depth < cardinality(walk.segments) \
                 ), resolved AS ( \
                     SELECT kind, path, node_id FROM walk \
                     WHERE depth = cardinality(segments) \
                 ) \
                 INSERT INTO node_link_refs ( \
                     space_id, source_node_id, reference_kind, target_path, \
                     target_node_id, occurrence_count \
                 ) \
                 SELECT $1, $2, requested.kind, requested.path, resolved.node_id, \
                        requested.count \
                 FROM requested \
                 LEFT JOIN resolved \
                   ON resolved.kind = requested.kind AND resolved.path = requested.path",
            )
            .bind(space_id)
            .bind(source_node_id)
            .bind(kinds)
            .bind(paths)
            .bind(counts)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        }

        sqlx::query(
            "INSERT INTO node_link_source_states ( \
                 space_id, source_node_id, source_content_sha256, source_path, \
                 parser_version, projected_at \
             ) VALUES ($1, $2, $3, $4, $5, now()) \
             ON CONFLICT (space_id, source_node_id) DO UPDATE \
             SET source_content_sha256 = EXCLUDED.source_content_sha256, \
                 source_path = EXCLUDED.source_path, \
                 parser_version = EXCLUDED.parser_version, \
                 projected_at = EXCLUDED.projected_at",
        )
        .bind(space_id)
        .bind(source_node_id)
        .bind(expected_content_sha256)
        .bind(expected_path)
        .bind(LINK_PARSER_VERSION)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if !commit_if_claim_owned(tx, fence).await? {
            return Ok(LinkSourceCommit::ClaimLost);
        }
        Ok(LinkSourceCommit::Applied)
    }

    pub async fn discard_source(
        &self,
        fence: &ClaimFence,
        space_id: Uuid,
        source_node_id: Uuid,
    ) -> Result<LinkSourceDiscard> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        if current_source_fingerprint(&mut tx, space_id, source_node_id)
            .await?
            .is_some()
        {
            if !commit_if_claim_owned(tx, fence).await? {
                return Ok(LinkSourceDiscard::ClaimLost);
            }
            return Ok(LinkSourceDiscard::Stale);
        }
        delete_source_projection(&mut tx, space_id, source_node_id).await?;
        if !commit_if_claim_owned(tx, fence).await? {
            return Ok(LinkSourceDiscard::ClaimLost);
        }
        Ok(LinkSourceDiscard::Deleted)
    }

    pub async fn outgoing(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
        limit: i64,
        cursor: Option<&OutgoingLinkCursor>,
    ) -> Result<Vec<StoredLinkReference>> {
        let rows = match cursor {
            Some(cursor) => {
                sqlx::query_as::<_, StoredLinkReferenceRow>(
                    "SELECT target_node_id, target_path, reference_kind, occurrence_count \
                     FROM node_link_refs \
                     WHERE space_id = $1 AND source_node_id = $2 \
                       AND (reference_kind, target_path) > ($3, $4) \
                     ORDER BY reference_kind, target_path \
                     LIMIT $5",
                )
                .bind(space_id)
                .bind(source_node_id)
                .bind(cursor.kind.as_str())
                .bind(&cursor.target_path)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query_as::<_, StoredLinkReferenceRow>(
                    "SELECT target_node_id, target_path, reference_kind, occurrence_count \
                     FROM node_link_refs \
                     WHERE space_id = $1 AND source_node_id = $2 \
                     ORDER BY reference_kind, target_path \
                     LIMIT $3",
                )
                .bind(space_id)
                .bind(source_node_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(map_sqlx_error)?;
        rows.into_iter()
            .map(StoredLinkReference::try_from)
            .collect()
    }

    pub async fn incoming(
        &self,
        space_id: Uuid,
        target_node_id: Uuid,
        limit: i64,
        cursor: Option<&IncomingLinkCursor>,
    ) -> Result<Vec<IncomingLinkReference>> {
        let rows = match cursor {
            Some(cursor) => {
                sqlx::query_as::<_, IncomingLinkReferenceRow>(
                    "SELECT refs.source_node_id, refs.reference_kind, refs.occurrence_count \
                     FROM node_link_refs refs \
                     JOIN nodes source \
                       ON source.id = refs.source_node_id \
                      AND source.space_id = refs.space_id \
                      AND source.deleted_at IS NULL \
                     WHERE refs.space_id = $1 AND refs.target_node_id = $2 \
                       AND (refs.source_node_id, refs.reference_kind) > ($3, $4) \
                     ORDER BY refs.source_node_id, refs.reference_kind \
                     LIMIT $5",
                )
                .bind(space_id)
                .bind(target_node_id)
                .bind(cursor.source_node_id)
                .bind(cursor.kind.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query_as::<_, IncomingLinkReferenceRow>(
                    "SELECT refs.source_node_id, refs.reference_kind, refs.occurrence_count \
                     FROM node_link_refs refs \
                     JOIN nodes source \
                       ON source.id = refs.source_node_id \
                      AND source.space_id = refs.space_id \
                      AND source.deleted_at IS NULL \
                     WHERE refs.space_id = $1 AND refs.target_node_id = $2 \
                     ORDER BY refs.source_node_id, refs.reference_kind \
                     LIMIT $3",
                )
                .bind(space_id)
                .bind(target_node_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(map_sqlx_error)?;
        rows.into_iter()
            .map(IncomingLinkReference::try_from)
            .collect()
    }

    pub async fn space_status(&self, space_id: Uuid) -> Result<SpaceLinkStatus> {
        sqlx::query_as(
            "WITH RECURSIVE live_paths AS ( \
                 SELECT node.id, node.parent_id, node.kind, '/'::text AS path \
                 FROM nodes node \
                 WHERE node.space_id = $1 AND node.parent_id IS NULL \
                   AND node.deleted_at IS NULL \
                 UNION ALL \
                 SELECT child.id, child.parent_id, child.kind, \
                        CASE WHEN parent.path = '/' THEN '/' || child.name \
                             ELSE parent.path || '/' || child.name END \
                 FROM nodes child \
                 JOIN live_paths parent ON parent.id = child.parent_id \
                 WHERE child.space_id = $1 AND child.deleted_at IS NULL \
             ), current_sources AS ( \
                 SELECT path.id AS source_node_id, text.content_sha256, path.path \
                 FROM live_paths path \
                 JOIN text_objects text \
                   ON text.space_id = $1 AND text.node_id = path.id \
                 WHERE path.kind = 'text' \
             ), invalid_reference_sources AS ( \
                 SELECT DISTINCT refs.source_node_id \
                 FROM node_link_refs refs \
                 LEFT JOIN live_paths target ON target.id = refs.target_node_id \
                 LEFT JOIN live_paths resolved \
                   ON refs.target_node_id IS NULL AND resolved.path = refs.target_path \
                 WHERE refs.space_id = $1 \
                   AND ( \
                       (refs.target_node_id IS NOT NULL \
                        AND (target.id IS NULL OR target.path <> refs.target_path)) \
                       OR (refs.target_node_id IS NULL AND resolved.id IS NOT NULL) \
                   ) \
             ), source_candidates AS ( \
                 SELECT source_node_id FROM current_sources \
                 UNION SELECT source_node_id FROM node_link_source_states WHERE space_id = $1 \
                 UNION SELECT source_node_id FROM node_link_refs WHERE space_id = $1 \
             ), source_jobs AS ( \
                 SELECT payload ->> 'source_node_id' AS source_node_id, \
                        true AS active, \
                        bool_or(status = 'running') AS syncing, \
                        bool_or(status = 'queued' AND failure_count > 0) AS retrying \
                 FROM background_jobs \
                 WHERE job_kind = 'node_link_source' \
                   AND status IN ('queued', 'running') \
                   AND payload ->> 'space_id' = $1::text \
                 GROUP BY payload ->> 'source_node_id' \
             ), space_jobs AS ( \
                 SELECT count(*) > 0 AS active, \
                        bool_or(status = 'running') AS syncing, \
                        bool_or(status = 'queued' AND failure_count > 0) AS retrying \
                 FROM background_jobs \
                 WHERE job_kind IN ('node_link_impact', 'node_link_space') \
                   AND status IN ('queued', 'running') \
                   AND payload ->> 'space_id' = $1::text \
             ), source_health AS ( \
                 SELECT candidate.source_node_id, \
                        current.source_node_id IS NULL \
                            OR state.source_node_id IS NULL \
                            OR state.source_content_sha256 <> current.content_sha256 \
                            OR state.source_path <> current.path \
                            OR state.parser_version <> $2 \
                            OR invalid.source_node_id IS NOT NULL AS stale, \
                        COALESCE(job.active, false) AS active, \
                        COALESCE(job.syncing, false) AS syncing, \
                        COALESCE(job.retrying, false) AS retrying, \
                        state.projected_at \
                 FROM source_candidates candidate \
                 LEFT JOIN current_sources current \
                   ON current.source_node_id = candidate.source_node_id \
                 LEFT JOIN node_link_source_states state \
                   ON state.space_id = $1 AND state.source_node_id = candidate.source_node_id \
                 LEFT JOIN invalid_reference_sources invalid \
                   ON invalid.source_node_id = candidate.source_node_id \
                 LEFT JOIN source_jobs job ON job.source_node_id = candidate.source_node_id::text \
             ) \
             SELECT \
                 count(*) FILTER (WHERE source.stale)::bigint AS outdated_documents, \
                 count(*) FILTER (WHERE source.retrying)::bigint AS retrying_documents, \
                 count(*) FILTER ( \
                     WHERE source.stale AND NOT source.active \
                       AND NOT COALESCE(space_job.active, false) \
                 )::bigint AS failed_documents, \
                 count(*) FILTER (WHERE source.active)::bigint AS active_documents, \
                 count(*) FILTER (WHERE source.syncing)::bigint AS syncing_documents, \
                 CASE WHEN EXISTS (SELECT 1 FROM current_sources) \
                      THEN max(source.projected_at) \
                      ELSE space_state.expanded_at \
                 END AS latest_index_update_at, \
                 COALESCE(space_job.active, false) AS space_pending, \
                 COALESCE(space_job.syncing, false) AS space_syncing, \
                 COALESCE(space_job.retrying, false) AS space_retrying, \
                 space_state.space_id IS NULL AND NOT COALESCE(space_job.active, false) \
                     AS space_failed \
             FROM spaces space \
             LEFT JOIN source_health source ON true \
             LEFT JOIN node_link_space_states space_state ON space_state.space_id = space.id \
             CROSS JOIN space_jobs space_job \
             WHERE space.id = $1 AND space.deleted_at IS NULL \
             GROUP BY space_state.space_id, space_state.expanded_at, \
                      space_job.active, space_job.syncing, space_job.retrying",
        )
        .bind(space_id)
        .bind(LINK_PARSER_VERSION)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
        .map(|status| status.unwrap_or_default())
    }
}

async fn commit_if_claim_owned(
    mut tx: Transaction<'_, Postgres>,
    fence: &ClaimFence,
) -> Result<bool> {
    if !JobQueue::owns_claim_in(&mut tx, fence)
        .await
        .map_err(map_job_queue_error)?
    {
        tx.rollback().await.map_err(map_sqlx_error)?;
        return Ok(false);
    }
    tx.commit().await.map_err(map_sqlx_error)?;
    Ok(true)
}

async fn notify_source_jobs(connection: &mut sqlx::PgConnection, inserted: i64) -> Result<()> {
    if inserted == 0 {
        return Ok(());
    }
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(BACKGROUND_JOB_NOTIFY_CHANNEL)
        .bind(LINK_SOURCE_JOB_KIND)
        .execute(connection)
        .await
        .map_err(map_sqlx_error)?;
    Ok(())
}

async fn delete_source_projection(
    connection: &mut sqlx::PgConnection,
    space_id: Uuid,
    source_node_id: Uuid,
) -> Result<()> {
    sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1 AND source_node_id = $2")
        .bind(space_id)
        .bind(source_node_id)
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    sqlx::query("DELETE FROM node_link_source_states WHERE space_id = $1 AND source_node_id = $2")
        .bind(space_id)
        .bind(source_node_id)
        .execute(connection)
        .await
        .map_err(map_sqlx_error)?;
    Ok(())
}

async fn current_source_fingerprint(
    connection: &mut sqlx::PgConnection,
    space_id: Uuid,
    source_node_id: Uuid,
) -> Result<Option<(String, String)>> {
    let live_space: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM spaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE")
            .bind(space_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(map_sqlx_error)?;
    if live_space.is_none() {
        return Ok(None);
    }
    let content_sha256: Option<String> = sqlx::query_scalar(
        "SELECT text.content_sha256 \
         FROM nodes node \
         JOIN text_objects text \
           ON text.node_id = node.id AND text.space_id = node.space_id \
         WHERE node.space_id = $1 AND node.id = $2 \
           AND node.kind = 'text' AND node.deleted_at IS NULL \
         FOR UPDATE OF node, text",
    )
    .bind(space_id)
    .bind(source_node_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    let Some(content_sha256) = content_sha256 else {
        return Ok(None);
    };
    let path: Option<String> = sqlx::query_scalar(
        "WITH RECURSIVE chain AS ( \
             SELECT id, parent_id, name, 0 AS depth \
             FROM nodes \
             WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
             UNION ALL \
             SELECT node.id, node.parent_id, node.name, chain.depth + 1 \
             FROM nodes node JOIN chain ON node.id = chain.parent_id \
             WHERE node.space_id = $1 AND node.deleted_at IS NULL \
         ) \
         SELECT CASE \
                  WHEN max(depth) = 0 THEN '/' \
                  ELSE '/' || string_agg(name, '/' ORDER BY depth DESC) \
                       FILTER (WHERE parent_id IS NOT NULL) \
                END \
         FROM chain",
    )
    .bind(space_id)
    .bind(source_node_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(map_sqlx_error)?
    .flatten();
    Ok(path.map(|path| (content_sha256, path)))
}

fn map_job_queue_error(error: notegate_jobs::JobQueueError) -> Error {
    Error::internal(format!("background job queue error: {error}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkExpansion {
    Expanded,
    Deleted,
    ClaimLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkSourceCommit {
    Applied,
    Deleted,
    Stale,
    ClaimLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkSourceDiscard {
    Deleted,
    Stale,
    ClaimLost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLinkReference {
    pub target_path: String,
    pub kind: LinkReferenceKind,
    pub occurrence_count: i32,
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

#[derive(Debug, Clone, Default, FromRow)]
pub struct SpaceLinkStatus {
    pub outdated_documents: i64,
    pub retrying_documents: i64,
    pub failed_documents: i64,
    pub active_documents: i64,
    pub syncing_documents: i64,
    pub latest_index_update_at: Option<DateTime<Utc>>,
    pub space_pending: bool,
    pub space_syncing: bool,
    pub space_retrying: bool,
    pub space_failed: bool,
}
