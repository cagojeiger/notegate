use chrono::{DateTime, Utc};
use notegate_core::{Error, Result};
use notegate_jobs::{ClaimFence, JobQueue};
use notegate_model::{
    IncomingLinkCursor, LinkReferenceKind, NodeLinkGraphState, NodeLinkGraphStatus,
    OutgoingLinkCursor,
};
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::files::queries::{node::derive_path, search::resolve_nodes_by_paths_with};
use crate::link_graph_work_repo::LINK_GRAPH_PROCESSOR_KIND;
use crate::map_sqlx_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkGraphStoredReference {
    pub target_path: String,
    pub kind: LinkReferenceKind,
    pub occurrence_count: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct LinkGraphSourceSnapshot<'a> {
    pub content_sha256: &'a str,
    pub path: &'a str,
    pub parser_version: i32,
    pub references: &'a [LinkGraphStoredReference],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkGraphOutgoingReference {
    pub target_node_id: Option<Uuid>,
    pub target_path: String,
    pub kind: LinkReferenceKind,
    pub occurrence_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkGraphIncomingReference {
    pub source_node_id: Uuid,
    pub kind: LinkReferenceKind,
    pub occurrence_count: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkGraphProjection {
    Applied { reference_count: usize },
    Failed,
    Removed,
    Skipped,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkGraphProjectionClaim {
    pub fence: ClaimFence,
    pub request_version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkSourceSnapshotState {
    Current,
    Changed,
    Missing,
}

#[derive(Debug, Clone)]
pub struct LinkGraphRepo {
    pool: PgPool,
}

impl LinkGraphRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn state(&self, space_id: Uuid, source_node_id: Uuid) -> Result<NodeLinkGraphState> {
        let row = sqlx::query_as::<_, NodeLinkGraphStateRow>(
            "SELECT source.projected_at, target.node_id IS NOT NULL AS target_exists, \
                    COALESCE(node.kind = 'text' AND node.deleted_at IS NULL, false) \
                        AS source_is_text, \
                    COALESCE(processor.processing_state = 'pending', false) \
                        AS processor_pending, \
                    target.request_version, target.active_job_id, \
                    target.active_request_version, target.failure_code, target.failed_at, \
                    job.status AS active_job_status, job.last_error_code AS active_job_error_code, \
                    job.completed_at AS active_job_completed_at \
             FROM (SELECT $1::uuid AS space_id, $2::uuid AS node_id) requested \
             LEFT JOIN nodes node \
               ON node.space_id = requested.space_id AND node.id = requested.node_id \
             LEFT JOIN node_link_source_states source \
               ON source.space_id = requested.space_id \
              AND source.source_node_id = requested.node_id \
             LEFT JOIN node_link_projection_targets target \
               ON target.space_id = requested.space_id AND target.node_id = requested.node_id \
             LEFT JOIN space_change_processor_states processor \
               ON processor.space_id = requested.space_id \
              AND processor.processor_kind = $3 \
             LEFT JOIN background_jobs job ON job.job_id = target.active_job_id",
        )
        .bind(space_id)
        .bind(source_node_id)
        .bind(LINK_GRAPH_PROCESSOR_KIND)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.into())
    }

    pub async fn outgoing(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
        limit: i64,
        cursor: Option<&OutgoingLinkCursor>,
    ) -> Result<Vec<LinkGraphOutgoingReference>> {
        let rows = match cursor {
            Some(cursor) => {
                sqlx::query_as::<_, OutgoingReferenceRow>(
                    "SELECT target_node_id, target_path, reference_kind, occurrence_count \
                     FROM node_link_refs \
                     WHERE space_id = $1 AND source_node_id = $2 \
                       AND (reference_kind, target_path) > ($3, $4) \
                     ORDER BY reference_kind, target_path LIMIT $5",
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
                sqlx::query_as::<_, OutgoingReferenceRow>(
                    "SELECT target_node_id, target_path, reference_kind, occurrence_count \
                     FROM node_link_refs \
                     WHERE space_id = $1 AND source_node_id = $2 \
                     ORDER BY reference_kind, target_path LIMIT $3",
                )
                .bind(space_id)
                .bind(source_node_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(map_sqlx_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn incoming(
        &self,
        space_id: Uuid,
        target_node_id: Uuid,
        limit: i64,
        cursor: Option<&IncomingLinkCursor>,
    ) -> Result<Vec<LinkGraphIncomingReference>> {
        let rows = match cursor {
            Some(cursor) => {
                sqlx::query_as::<_, IncomingReferenceRow>(
                    "SELECT refs.source_node_id, refs.reference_kind, refs.occurrence_count \
                     FROM node_link_refs refs \
                     JOIN nodes source ON source.id = refs.source_node_id \
                       AND source.space_id = refs.space_id AND source.deleted_at IS NULL \
                     WHERE refs.space_id = $1 AND refs.target_node_id = $2 \
                       AND (refs.source_node_id, refs.reference_kind) > ($3, $4) \
                     ORDER BY refs.source_node_id, refs.reference_kind LIMIT $5",
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
                sqlx::query_as::<_, IncomingReferenceRow>(
                    "SELECT refs.source_node_id, refs.reference_kind, refs.occurrence_count \
                     FROM node_link_refs refs \
                     JOIN nodes source ON source.id = refs.source_node_id \
                       AND source.space_id = refs.space_id AND source.deleted_at IS NULL \
                     WHERE refs.space_id = $1 AND refs.target_node_id = $2 \
                     ORDER BY refs.source_node_id, refs.reference_kind LIMIT $3",
                )
                .bind(space_id)
                .bind(target_node_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(map_sqlx_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn replace_source(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
        claim: LinkGraphProjectionClaim,
        source: LinkGraphSourceSnapshot<'_>,
    ) -> Result<LinkGraphProjection> {
        let LinkGraphSourceSnapshot {
            content_sha256: expected_content_sha256,
            path: expected_source_path,
            parser_version,
            references,
        } = source;
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        if let Some(projection) = prepare_plain_projection_in(
            &mut tx,
            space_id,
            source_node_id,
            claim,
            expected_content_sha256,
            expected_source_path,
        )
        .await?
        {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(projection);
        }

        let target_paths = references
            .iter()
            .map(|reference| reference.target_path.clone())
            .collect::<Vec<_>>();
        let target_ids = resolve_nodes_by_paths_with(&mut *tx, space_id, &target_paths)
            .await?
            .into_iter()
            .map(|(_index, path, node)| (path, node.id))
            .collect::<std::collections::HashMap<_, _>>();
        let mut resolved_target_ids = target_ids.values().copied().collect::<Vec<_>>();
        resolved_target_ids.sort_unstable();
        resolved_target_ids.dedup();
        let locked_target_ids =
            lock_live_reference_targets_in(&mut tx, space_id, &resolved_target_ids).await?;
        if !lock_projection_target_in(&mut tx, space_id, source_node_id, claim).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Stale);
        }

        sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1 AND source_node_id = $2")
            .bind(space_id)
            .bind(source_node_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

        if !references.is_empty() {
            let resolved_ids = references
                .iter()
                .map(|reference| {
                    target_ids
                        .get(&reference.target_path)
                        .filter(|node_id| locked_target_ids.binary_search(*node_id).is_ok())
                        .copied()
                })
                .collect::<Vec<_>>();
            let kinds = references
                .iter()
                .map(|reference| reference.kind.as_str())
                .collect::<Vec<_>>();
            let counts = references
                .iter()
                .map(|reference| reference.occurrence_count)
                .collect::<Vec<_>>();
            sqlx::query(
                "INSERT INTO node_link_refs ( \
                     space_id, source_node_id, target_node_id, target_path, \
                     reference_kind, occurrence_count \
                 ) \
                 SELECT $1, $2, item.target_node_id, item.target_path, \
                        item.reference_kind, item.occurrence_count \
                 FROM unnest($3::uuid[], $4::text[], $5::text[], $6::int[]) \
                      AS item(target_node_id, target_path, reference_kind, occurrence_count)",
            )
            .bind(space_id)
            .bind(source_node_id)
            .bind(resolved_ids)
            .bind(target_paths)
            .bind(kinds)
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
                 projected_at = now()",
        )
        .bind(space_id)
        .bind(source_node_id)
        .bind(expected_content_sha256)
        .bind(expected_source_path)
        .bind(parser_version)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        complete_projection_target_in(&mut tx, space_id, source_node_id, claim).await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(LinkGraphProjection::Applied {
            reference_count: references.len(),
        })
    }

    pub async fn fail_projection_target(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
        claim: LinkGraphProjectionClaim,
        failure_code: &str,
        expected_content_sha256: &str,
        expected_source_path: &str,
    ) -> Result<LinkGraphProjection> {
        if failure_code.is_empty() || failure_code.len() > 128 {
            return Err(Error::validation("invalid link projection failure code"));
        }

        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        if let Some(projection) = prepare_plain_projection_in(
            &mut tx,
            space_id,
            source_node_id,
            claim,
            expected_content_sha256,
            expected_source_path,
        )
        .await?
        {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(projection);
        }
        if !lock_projection_target_in(&mut tx, space_id, source_node_id, claim).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Stale);
        }
        let affected = sqlx::query(
            "UPDATE node_link_projection_targets \
             SET active_job_id = NULL, active_request_version = NULL, \
                 failure_code = $5, failed_at = now(), updated_at = now() \
             WHERE space_id = $1 AND node_id = $2 \
               AND active_job_id = $3 AND active_request_version = $4 \
               AND request_version = $4",
        )
        .bind(space_id)
        .bind(source_node_id)
        .bind(claim.fence.job_id)
        .bind(claim.request_version)
        .bind(failure_code)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected();
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(if affected == 1 {
            LinkGraphProjection::Failed
        } else {
            LinkGraphProjection::Stale
        })
    }

    pub async fn reconcile_non_text_node(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        claim: LinkGraphProjectionClaim,
    ) -> Result<LinkGraphProjection> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        if !owns_projection_claim_in(&mut tx, claim).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Stale);
        }
        let Some(space_live) = space_state(&mut tx, space_id).await? else {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Removed);
        };
        if !space_live {
            if !lock_projection_target_in(&mut tx, space_id, node_id, claim).await? {
                tx.commit().await.map_err(map_sqlx_error)?;
                return Ok(LinkGraphProjection::Stale);
            }
            complete_projection_target_in(&mut tx, space_id, node_id, claim).await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Removed);
        }
        let state: Option<(String, bool)> = sqlx::query_as(
            "SELECT kind, deleted_at IS NOT NULL \
             FROM nodes WHERE space_id = $1 AND id = $2 FOR NO KEY UPDATE",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if !lock_projection_target_in(&mut tx, space_id, node_id, claim).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Stale);
        }

        match state {
            Some((kind, false)) if kind == "text" => {
                complete_projection_target_in(&mut tx, space_id, node_id, claim).await?;
                tx.commit().await.map_err(map_sqlx_error)?;
                return Ok(LinkGraphProjection::Stale);
            }
            Some((_kind, false)) => {
                cleanup_source_in(&mut tx, space_id, node_id).await?;
            }
            Some((_, true)) | None => {
                cleanup_deleted_node_in(&mut tx, space_id, node_id).await?;
            }
        }
        complete_projection_target_in(&mut tx, space_id, node_id, claim).await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(LinkGraphProjection::Removed)
    }

    pub async fn cleanup_encrypted_source(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
        claim: LinkGraphProjectionClaim,
        expected_content_sha256: &str,
    ) -> Result<LinkGraphProjection> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        if !owns_projection_claim_in(&mut tx, claim).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Stale);
        }
        let Some(space_live) = space_state(&mut tx, space_id).await? else {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Removed);
        };
        if !space_live {
            if !lock_projection_target_in(&mut tx, space_id, source_node_id, claim).await? {
                tx.commit().await.map_err(map_sqlx_error)?;
                return Ok(LinkGraphProjection::Stale);
            }
            complete_projection_target_in(&mut tx, space_id, source_node_id, claim).await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Removed);
        }
        let current: Option<(String, String)> = sqlx::query_as(
            "SELECT text.content_sha256, text.storage_format \
             FROM nodes node \
             JOIN text_objects text ON text.node_id = node.id AND text.space_id = node.space_id \
             WHERE node.space_id = $1 AND node.id = $2 \
               AND node.kind = 'text' AND node.deleted_at IS NULL \
             FOR NO KEY UPDATE OF node, text",
        )
        .bind(space_id)
        .bind(source_node_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if !lock_projection_target_in(&mut tx, space_id, source_node_id, claim).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Stale);
        }

        match current {
            Some((content_sha256, storage_format))
                if storage_format == "encrypted" && content_sha256 == expected_content_sha256 =>
            {
                cleanup_source_in(&mut tx, space_id, source_node_id).await?;
                complete_projection_target_in(&mut tx, space_id, source_node_id, claim).await?;
                tx.commit().await.map_err(map_sqlx_error)?;
                Ok(LinkGraphProjection::Skipped)
            }
            None => {
                cleanup_deleted_node_in(&mut tx, space_id, source_node_id).await?;
                complete_projection_target_in(&mut tx, space_id, source_node_id, claim).await?;
                tx.commit().await.map_err(map_sqlx_error)?;
                Ok(LinkGraphProjection::Removed)
            }
            Some((_content_sha256, _storage_format)) => {
                complete_projection_target_in(&mut tx, space_id, source_node_id, claim).await?;
                tx.commit().await.map_err(map_sqlx_error)?;
                Ok(LinkGraphProjection::Stale)
            }
        }
    }
}

#[derive(Debug, FromRow)]
struct OutgoingReferenceRow {
    target_node_id: Option<Uuid>,
    target_path: String,
    reference_kind: String,
    occurrence_count: i32,
}

impl TryFrom<OutgoingReferenceRow> for LinkGraphOutgoingReference {
    type Error = Error;

    fn try_from(row: OutgoingReferenceRow) -> Result<Self> {
        Ok(Self {
            target_node_id: row.target_node_id,
            target_path: row.target_path,
            kind: parse_reference_kind(&row.reference_kind)?,
            occurrence_count: row.occurrence_count,
        })
    }
}

#[derive(Debug, FromRow)]
struct IncomingReferenceRow {
    source_node_id: Uuid,
    reference_kind: String,
    occurrence_count: i32,
}

#[derive(Debug, FromRow)]
struct NodeLinkGraphStateRow {
    projected_at: Option<DateTime<Utc>>,
    target_exists: bool,
    source_is_text: bool,
    processor_pending: bool,
    request_version: Option<i64>,
    active_job_id: Option<Uuid>,
    active_request_version: Option<i64>,
    failure_code: Option<String>,
    failed_at: Option<DateTime<Utc>>,
    active_job_status: Option<String>,
    active_job_error_code: Option<String>,
    active_job_completed_at: Option<DateTime<Utc>>,
}

impl From<NodeLinkGraphStateRow> for NodeLinkGraphState {
    fn from(row: NodeLinkGraphStateRow) -> Self {
        let active_job_is_current = row.active_job_id.is_some()
            && row.active_request_version.is_some()
            && row.active_request_version == row.request_version;
        let active_job_is_terminal = active_job_is_current
            && matches!(row.active_job_status.as_deref(), Some("succeeded" | "dead"));
        let terminal_failure_code = match row.active_job_status.as_deref() {
            Some("dead") if active_job_is_terminal => Some(
                row.active_job_error_code
                    .unwrap_or_else(|| "job_failed".to_owned()),
            ),
            Some("succeeded") if active_job_is_terminal => Some("projection_incomplete".to_owned()),
            _ => None,
        };
        let stored_failure_code = row.failure_code.or(terminal_failure_code);
        let stored_failed_at = row.failed_at.or(if active_job_is_terminal {
            row.active_job_completed_at
        } else {
            None
        });
        let status = if active_job_is_current && !active_job_is_terminal {
            NodeLinkGraphStatus::Syncing
        } else if row.source_is_text && row.processor_pending {
            NodeLinkGraphStatus::Pending
        } else if stored_failure_code.is_some() {
            NodeLinkGraphStatus::Failed
        } else if row.target_exists {
            NodeLinkGraphStatus::Pending
        } else {
            NodeLinkGraphStatus::Idle
        };
        let (failure_code, failed_at) = if status == NodeLinkGraphStatus::Failed {
            (stored_failure_code, stored_failed_at)
        } else {
            (None, None)
        };
        Self {
            status,
            projected_at: row.projected_at,
            failure_code,
            failed_at,
        }
    }
}

impl TryFrom<IncomingReferenceRow> for LinkGraphIncomingReference {
    type Error = Error;

    fn try_from(row: IncomingReferenceRow) -> Result<Self> {
        Ok(Self {
            source_node_id: row.source_node_id,
            kind: parse_reference_kind(&row.reference_kind)?,
            occurrence_count: row.occurrence_count,
        })
    }
}

fn parse_reference_kind(value: &str) -> Result<LinkReferenceKind> {
    LinkReferenceKind::parse(value)
        .ok_or_else(|| Error::internal(format!("unknown link reference kind: {value}")))
}

async fn prepare_plain_projection_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    source_node_id: Uuid,
    claim: LinkGraphProjectionClaim,
    expected_content_sha256: &str,
    expected_source_path: &str,
) -> Result<Option<LinkGraphProjection>> {
    if !owns_projection_claim_in(connection, claim).await? {
        return Ok(Some(LinkGraphProjection::Stale));
    }
    let Some(space_live) = space_state(connection, space_id).await? else {
        return Ok(Some(LinkGraphProjection::Removed));
    };
    if !space_live {
        if !lock_projection_target_in(connection, space_id, source_node_id, claim).await? {
            return Ok(Some(LinkGraphProjection::Stale));
        }
        complete_projection_target_in(connection, space_id, source_node_id, claim).await?;
        return Ok(Some(LinkGraphProjection::Removed));
    }

    let source_state = link_source_snapshot_state_in(
        connection,
        space_id,
        source_node_id,
        expected_content_sha256,
        expected_source_path,
    )
    .await?;
    if source_state == LinkSourceSnapshotState::Current {
        return Ok(None);
    }
    if !lock_projection_target_in(connection, space_id, source_node_id, claim).await? {
        return Ok(Some(LinkGraphProjection::Stale));
    }
    match source_state {
        LinkSourceSnapshotState::Current => Ok(None),
        LinkSourceSnapshotState::Changed => {
            complete_projection_target_in(connection, space_id, source_node_id, claim).await?;
            Ok(Some(LinkGraphProjection::Stale))
        }
        LinkSourceSnapshotState::Missing => {
            cleanup_deleted_node_in(connection, space_id, source_node_id).await?;
            complete_projection_target_in(connection, space_id, source_node_id, claim).await?;
            Ok(Some(LinkGraphProjection::Removed))
        }
    }
}

async fn link_source_snapshot_state_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    source_node_id: Uuid,
    expected_content_sha256: &str,
    expected_source_path: &str,
) -> Result<LinkSourceSnapshotState> {
    let current: Option<(String, String)> = sqlx::query_as(
        "SELECT text.content_sha256, text.storage_format \
         FROM nodes node \
         JOIN text_objects text ON text.node_id = node.id AND text.space_id = node.space_id \
         WHERE node.space_id = $1 AND node.id = $2 \
           AND node.kind = 'text' AND node.deleted_at IS NULL \
         FOR NO KEY UPDATE OF node, text",
    )
    .bind(space_id)
    .bind(source_node_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    let Some((content_sha256, storage_format)) = current else {
        return Ok(LinkSourceSnapshotState::Missing);
    };
    if storage_format != "plain" || content_sha256 != expected_content_sha256 {
        return Ok(LinkSourceSnapshotState::Changed);
    }
    let source_path = derive_path(&mut *connection, space_id, source_node_id)
        .await?
        .ok_or_else(|| Error::internal("live link source has no path"))?;
    Ok(if source_path == expected_source_path {
        LinkSourceSnapshotState::Current
    } else {
        LinkSourceSnapshotState::Changed
    })
}

async fn space_state(connection: &mut PgConnection, space_id: Uuid) -> Result<Option<bool>> {
    sqlx::query_scalar("SELECT deleted_at IS NULL FROM spaces WHERE id = $1")
        .bind(space_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(map_sqlx_error)
}

async fn lock_live_reference_targets_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_ids: &[Uuid],
) -> Result<Vec<Uuid>> {
    if node_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_scalar(
        "SELECT id FROM nodes \
         WHERE space_id = $1 AND id = ANY($2) AND deleted_at IS NULL \
         ORDER BY id FOR KEY SHARE",
    )
    .bind(space_id)
    .bind(node_ids)
    .fetch_all(&mut *connection)
    .await
    .map_err(map_sqlx_error)
}

async fn owns_projection_claim_in(
    connection: &mut PgConnection,
    claim: LinkGraphProjectionClaim,
) -> Result<bool> {
    JobQueue::owns_claim_in(connection, &claim.fence)
        .await
        .map_err(|error| Error::internal(format!("link graph job claim check failed: {error}")))
}

async fn lock_projection_target_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
    claim: LinkGraphProjectionClaim,
) -> Result<bool> {
    let request_version: Option<i64> = sqlx::query_scalar(
        "SELECT request_version FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2 AND active_job_id = $3 \
           AND active_request_version = $4 \
         FOR UPDATE",
    )
    .bind(space_id)
    .bind(node_id)
    .bind(claim.fence.job_id)
    .bind(claim.request_version)
    .fetch_optional(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    match request_version {
        Some(request_version) if request_version == claim.request_version => Ok(true),
        Some(_request_version) => {
            sqlx::query(
                "UPDATE node_link_projection_targets \
                 SET active_job_id = NULL, active_request_version = NULL, \
                     updated_at = now() \
                 WHERE space_id = $1 AND node_id = $2 AND active_job_id = $3",
            )
            .bind(space_id)
            .bind(node_id)
            .bind(claim.fence.job_id)
            .execute(&mut *connection)
            .await
            .map_err(map_sqlx_error)?;
            Ok(false)
        }
        None => Ok(false),
    }
}

async fn complete_projection_target_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
    claim: LinkGraphProjectionClaim,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2 \
           AND active_job_id = $3 AND active_request_version = $4 \
           AND request_version = $4",
    )
    .bind(space_id)
    .bind(node_id)
    .bind(claim.fence.job_id)
    .bind(claim.request_version)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

async fn cleanup_deleted_node_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<()> {
    cleanup_source_in(connection, space_id, node_id).await?;
    clear_target_in(connection, space_id, node_id).await
}

async fn cleanup_source_in(
    connection: &mut PgConnection,
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
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    Ok(())
}

async fn clear_target_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    target_node_id: Uuid,
) -> Result<()> {
    sqlx::query(
        "UPDATE node_link_refs SET target_node_id = NULL \
         WHERE space_id = $1 AND target_node_id = $2",
    )
    .bind(space_id)
    .bind(target_node_id)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}
