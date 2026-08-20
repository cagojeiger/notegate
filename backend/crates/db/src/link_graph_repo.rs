use chrono::{DateTime, Utc};
use notegate_core::{Error, Result};
use notegate_jobs::{ClaimFence, JobQueue};
use notegate_model::{
    IncomingLinkCursor, LinkReferenceKind, NodeLinkGraphState, NodeLinkGraphStatus,
    OutgoingLinkCursor,
};
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::files::queries::{node::derive_path, search::resolve_node_ids_by_paths_with};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkGraphNodeReadModel {
    pub state: NodeLinkGraphState,
    pub node_exists: bool,
    pub source_indexable: bool,
    pub request_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkGraphProjection {
    Applied,
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
        Ok(self.node_state_row(space_id, source_node_id).await?.into())
    }

    pub async fn node_read_model(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
    ) -> Result<LinkGraphNodeReadModel> {
        let row = self.node_state_row(space_id, source_node_id).await?;
        let node_exists = row.node_exists;
        let source_indexable = row.source_indexable;
        let request_pending = row.request_pending();
        Ok(LinkGraphNodeReadModel {
            state: row.into(),
            node_exists,
            source_indexable,
            request_pending,
        })
    }

    async fn node_state_row(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
    ) -> Result<NodeLinkGraphStateRow> {
        sqlx::query_as::<_, NodeLinkGraphStateRow>(
            "SELECT projection.projected_at, \
                    COALESCE(projection.needs_projection, false) AS needs_projection, \
                    (COALESCE(space_state.available_at IS NOT NULL, false) OR EXISTS ( \
                        SELECT 1 FROM node_link_projections pending_projection \
                        WHERE pending_projection.space_id = requested.space_id \
                          AND pending_projection.needs_projection \
                    )) AS space_pending, \
                    projection.request_version, projection.active_job_id, \
                    projection.active_request_version, projection.failure_code, \
                    projection.failed_at, \
                    job.status AS active_job_status, job.last_error_code AS active_job_error_code, \
                    job.completed_at AS active_job_completed_at, \
                    node.id IS NOT NULL AS node_exists, \
                    COALESCE(text.storage_format = 'plain', false) AS source_indexable \
             FROM (SELECT $1::uuid AS space_id, $2::uuid AS node_id) requested \
             LEFT JOIN nodes node \
               ON node.space_id = requested.space_id AND node.id = requested.node_id \
              AND node.deleted_at IS NULL \
             LEFT JOIN text_objects text \
               ON text.space_id = node.space_id AND text.node_id = node.id \
              AND node.kind = 'text' \
             LEFT JOIN node_link_projections projection \
               ON projection.space_id = requested.space_id \
              AND projection.source_node_id = requested.node_id \
             LEFT JOIN link_graph_space_states space_state \
               ON space_state.space_id = requested.space_id \
             LEFT JOIN background_jobs job ON job.job_id = projection.active_job_id",
        )
        .bind(space_id)
        .bind(source_node_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)
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
        let target_ids = resolve_node_ids_by_paths_with(&mut *tx, space_id, &target_paths)
            .await?
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        let mut resolved_target_ids = target_ids.values().copied().collect::<Vec<_>>();
        resolved_target_ids.sort_unstable();
        resolved_target_ids.dedup();
        let locked_target_ids =
            lock_live_reference_targets_in(&mut tx, space_id, &resolved_target_ids).await?;
        if !lock_owned_projection_target_in(&mut tx, space_id, source_node_id, claim).await? {
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

        mark_projection_applied_in(&mut tx, space_id, source_node_id, claim).await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(LinkGraphProjection::Applied)
    }

    pub async fn settle_stale_target(
        &self,
        space_id: Uuid,
        source_node_id: Uuid,
        claim: LinkGraphProjectionClaim,
    ) -> Result<LinkGraphProjection> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        if !lock_owned_projection_target_in(&mut tx, space_id, source_node_id, claim).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Stale);
        }
        settle_stale_projection_in(&mut tx, space_id, source_node_id, claim).await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(LinkGraphProjection::Stale)
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
        if !lock_owned_projection_target_in(&mut tx, space_id, source_node_id, claim).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Stale);
        }
        let affected = sqlx::query(
            "UPDATE node_link_projections \
             SET needs_projection = false, active_job_id = NULL, active_request_version = NULL, \
                 failure_code = $5, failed_at = now() \
             WHERE space_id = $1 AND source_node_id = $2 \
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
        let space_live = space_state(&mut tx, space_id).await? == Some(true);
        let state: Option<(String, bool)> = if space_live {
            sqlx::query_as(
                "SELECT kind, deleted_at IS NOT NULL \
                 FROM nodes WHERE space_id = $1 AND id = $2 FOR NO KEY UPDATE",
            )
            .bind(space_id)
            .bind(node_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?
        } else {
            None
        };
        if !lock_owned_projection_target_in(&mut tx, space_id, node_id, claim).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Stale);
        }
        if !space_live {
            remove_projection_in(&mut tx, space_id, node_id, claim).await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Removed);
        }

        match state {
            Some((kind, false)) if kind == "text" => {
                settle_stale_projection_in(&mut tx, space_id, node_id, claim).await?;
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
        remove_projection_in(&mut tx, space_id, node_id, claim).await?;
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
        let space_live = space_state(&mut tx, space_id).await? == Some(true);
        let current: Option<(String, String)> = if space_live {
            sqlx::query_as(
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
            .map_err(map_sqlx_error)?
        } else {
            None
        };
        if !lock_owned_projection_target_in(&mut tx, space_id, source_node_id, claim).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Stale);
        }
        if !space_live {
            remove_projection_in(&mut tx, space_id, source_node_id, claim).await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphProjection::Removed);
        }

        match current {
            Some((content_sha256, storage_format))
                if storage_format == "encrypted" && content_sha256 == expected_content_sha256 =>
            {
                cleanup_source_in(&mut tx, space_id, source_node_id).await?;
                remove_projection_in(&mut tx, space_id, source_node_id, claim).await?;
                tx.commit().await.map_err(map_sqlx_error)?;
                Ok(LinkGraphProjection::Skipped)
            }
            None => {
                cleanup_deleted_node_in(&mut tx, space_id, source_node_id).await?;
                remove_projection_in(&mut tx, space_id, source_node_id, claim).await?;
                tx.commit().await.map_err(map_sqlx_error)?;
                Ok(LinkGraphProjection::Removed)
            }
            Some((_content_sha256, _storage_format)) => {
                settle_stale_projection_in(&mut tx, space_id, source_node_id, claim).await?;
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
    needs_projection: bool,
    space_pending: bool,
    request_version: Option<i64>,
    active_job_id: Option<Uuid>,
    active_request_version: Option<i64>,
    failure_code: Option<String>,
    failed_at: Option<DateTime<Utc>>,
    active_job_status: Option<String>,
    active_job_error_code: Option<String>,
    active_job_completed_at: Option<DateTime<Utc>>,
    node_exists: bool,
    source_indexable: bool,
}

impl NodeLinkGraphStateRow {
    fn request_pending(&self) -> bool {
        self.needs_projection
            && ((self.active_job_id.is_none() && self.failed_at.is_none())
                || matches!(
                    self.active_job_status.as_deref(),
                    Some("queued" | "running" | "succeeded")
                )
                || (self.active_job_status.as_deref() == Some("dead")
                    && self.active_request_version != self.request_version))
    }
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
        } else if stored_failure_code.is_some() {
            NodeLinkGraphStatus::Failed
        } else if row.needs_projection {
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
            space_pending: row.space_pending,
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
    if space_state(connection, space_id).await? != Some(true) {
        if !lock_owned_projection_target_in(connection, space_id, source_node_id, claim).await? {
            return Ok(Some(LinkGraphProjection::Stale));
        }
        remove_projection_in(connection, space_id, source_node_id, claim).await?;
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
    if !lock_owned_projection_target_in(connection, space_id, source_node_id, claim).await? {
        return Ok(Some(LinkGraphProjection::Stale));
    }
    match source_state {
        LinkSourceSnapshotState::Current => Ok(None),
        LinkSourceSnapshotState::Changed => {
            settle_stale_projection_in(connection, space_id, source_node_id, claim).await?;
            Ok(Some(LinkGraphProjection::Stale))
        }
        LinkSourceSnapshotState::Missing => {
            cleanup_deleted_node_in(connection, space_id, source_node_id).await?;
            remove_projection_in(connection, space_id, source_node_id, claim).await?;
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

async fn lock_owned_projection_target_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
    claim: LinkGraphProjectionClaim,
) -> Result<bool> {
    // Keep the shared job row last so a blocked source cannot stall its peers.
    let request_version: Option<i64> = sqlx::query_scalar(
        "SELECT request_version FROM node_link_projections \
         WHERE space_id = $1 AND source_node_id = $2 AND active_job_id = $3 \
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
    let Some(request_version) = request_version else {
        return Ok(false);
    };
    if !owns_projection_claim_in(connection, claim).await? {
        return Ok(false);
    }
    if request_version == claim.request_version {
        Ok(true)
    } else {
        sqlx::query(
            "UPDATE node_link_projections \
                 SET active_job_id = NULL, active_request_version = NULL \
                 WHERE space_id = $1 AND source_node_id = $2 AND active_job_id = $3",
        )
        .bind(space_id)
        .bind(node_id)
        .bind(claim.fence.job_id)
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
        Ok(false)
    }
}

async fn mark_projection_applied_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
    claim: LinkGraphProjectionClaim,
) -> Result<()> {
    sqlx::query(
        "UPDATE node_link_projections \
         SET projected_at = now(), needs_projection = false, \
             active_job_id = NULL, active_request_version = NULL, \
             failure_code = NULL, failed_at = NULL \
         WHERE space_id = $1 AND source_node_id = $2 \
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

async fn settle_stale_projection_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
    claim: LinkGraphProjectionClaim,
) -> Result<()> {
    sqlx::query(
        "UPDATE node_link_projections \
         SET needs_projection = false, active_job_id = NULL, \
             active_request_version = NULL \
         WHERE space_id = $1 AND source_node_id = $2 \
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

async fn remove_projection_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
    claim: LinkGraphProjectionClaim,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM node_link_projections \
         WHERE space_id = $1 AND source_node_id = $2 \
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
