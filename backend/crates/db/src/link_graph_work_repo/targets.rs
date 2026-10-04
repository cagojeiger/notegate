//! Durable per-node requests, pending/claimed reads, and terminal-job settlement.

use notegate_core::Result;
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use super::{LINK_GRAPH_PROJECT_BATCH_MAX, LinkGraphProjectionTarget, LinkGraphWorkRepo};
use crate::link_graph_state::NODE_REQUEST_PENDING_PREDICATE;
use crate::map_sqlx_error;

impl LinkGraphWorkRepo {
    pub async fn claimed_targets(
        &self,
        job_id: Uuid,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<Vec<LinkGraphProjectionTarget>> {
        validate_node_batch(node_ids)?;
        sqlx::query_as::<_, ProjectionTargetRow>(
            "SELECT source_node_id AS node_id, active_request_version AS request_version \
             FROM node_link_projections \
             WHERE active_job_id = $1 AND active_request_version IS NOT NULL \
               AND space_id = $2 AND source_node_id = ANY($3) \
             ORDER BY source_node_id",
        )
        .bind(job_id)
        .bind(space_id)
        .bind(node_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)
        .map(|rows| rows.into_iter().map(Into::into).collect())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct SettlementSummary {
    pub(super) failed: usize,
    pub(super) has_more: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum TargetScope<'a> {
    All,
    Space(Uuid),
    Nodes {
        space_id: Uuid,
        node_ids: &'a [Uuid],
    },
}

#[derive(Debug, FromRow)]
struct ProjectionTargetRow {
    node_id: Uuid,
    request_version: i64,
}

impl From<ProjectionTargetRow> for LinkGraphProjectionTarget {
    fn from(row: ProjectionTargetRow) -> Self {
        Self {
            node_id: row.node_id,
            request_version: row.request_version,
        }
    }
}

pub(super) async fn stage_node_ids_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_ids: &[Uuid],
    supersede_active_job: bool,
) -> Result<usize> {
    if node_ids.is_empty() {
        return Ok(0);
    }
    let affected = sqlx::query(
        "WITH input AS ( \
             SELECT DISTINCT requested.node_id \
             FROM unnest($2::uuid[]) AS requested(node_id) \
         ), candidates AS ( \
             SELECT input.node_id \
             FROM input \
             WHERE EXISTS ( \
                 SELECT 1 FROM nodes node \
                 WHERE node.id = input.node_id AND node.space_id = $1 \
             ) OR EXISTS ( \
                 SELECT 1 FROM node_link_projections projection \
                 WHERE projection.space_id = $1 \
                   AND projection.source_node_id = input.node_id \
             ) \
         ) \
         INSERT INTO node_link_projections ( \
             space_id, source_node_id, needs_projection, request_version \
         ) \
         SELECT $1, candidates.node_id, true, 1 \
         FROM candidates \
         ON CONFLICT (space_id, source_node_id) DO UPDATE \
         SET needs_projection = true, \
             request_version = node_link_projections.request_version + 1, \
             active_job_id = CASE WHEN $3 THEN NULL \
                 ELSE node_link_projections.active_job_id END, \
             active_request_version = CASE WHEN $3 THEN NULL \
                 ELSE node_link_projections.active_request_version END, \
             failure_code = NULL, failed_at = NULL",
    )
    .bind(space_id)
    .bind(node_ids)
    .bind(supersede_active_job)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();
    usize::try_from(affected)
        .map_err(|_error| notegate_core::Error::internal("link target count overflow"))
}

pub(super) async fn settle_terminal_targets_in(
    connection: &mut PgConnection,
    scope: TargetScope<'_>,
    limit: i64,
) -> Result<SettlementSummary> {
    let (space_id, node_ids) = scope_parameters(scope);
    let (processed, failed): (i64, i64) = sqlx::query_as(
        "WITH candidates AS ( \
             SELECT projection.space_id, projection.source_node_id, \
                    projection.request_version, projection.active_request_version, job.status, \
                    job.last_error_code, job.completed_at \
             FROM node_link_projections projection \
             JOIN background_jobs job ON job.job_id = projection.active_job_id \
             WHERE ($1::uuid IS NULL OR projection.space_id = $1) \
               AND ($2::uuid[] IS NULL OR projection.source_node_id = ANY($2)) \
               AND job.status IN ('succeeded', 'dead') \
             ORDER BY projection.space_id, projection.source_node_id \
             LIMIT $3 FOR UPDATE OF projection SKIP LOCKED \
         ), updated AS ( \
             UPDATE node_link_projections projection \
             SET active_job_id = NULL, active_request_version = NULL, \
                 needs_projection = \
                     candidate.active_request_version IS DISTINCT FROM candidate.request_version, \
                 failure_code = CASE \
                     WHEN candidate.active_request_version IS DISTINCT FROM candidate.request_version \
                         THEN NULL \
                     WHEN candidate.status = 'dead' \
                         THEN COALESCE(candidate.last_error_code, 'job_failed') \
                     ELSE 'projection_incomplete' \
                 END, \
                 failed_at = CASE \
                     WHEN candidate.active_request_version IS DISTINCT FROM candidate.request_version \
                         THEN NULL \
                     ELSE COALESCE(candidate.completed_at, now()) \
                 END \
             FROM candidates candidate \
             WHERE projection.space_id = candidate.space_id \
               AND projection.source_node_id = candidate.source_node_id \
             RETURNING projection.failure_code IS NOT NULL AS failed \
         ) \
         SELECT count(*), count(*) FILTER (WHERE failed) FROM updated",
    )
    .bind(space_id)
    .bind(node_ids)
    .bind(limit)
    .fetch_one(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    Ok(SettlementSummary {
        failed: usize::try_from(failed).map_err(|_error| {
            notegate_core::Error::internal("failed link target count overflow")
        })?,
        has_more: processed == limit,
    })
}

pub(super) fn scope_parameters(scope: TargetScope<'_>) -> (Option<Uuid>, Option<Vec<Uuid>>) {
    match scope {
        TargetScope::All => (None, None),
        TargetScope::Space(space_id) => (Some(space_id), None),
        TargetScope::Nodes { space_id, node_ids } => (Some(space_id), Some(node_ids.to_vec())),
    }
}

pub(super) fn validate_node_batch(node_ids: &[Uuid]) -> Result<()> {
    if node_ids.is_empty() || node_ids.len() > LINK_GRAPH_PROJECT_BATCH_MAX {
        return Err(notegate_core::Error::validation(format!(
            "link graph batch must contain between 1 and {LINK_GRAPH_PROJECT_BATCH_MAX} node ids"
        )));
    }
    Ok(())
}

pub(super) async fn space_pending_in(
    connection: &mut PgConnection,
    space_id: Uuid,
) -> Result<Option<bool>> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT ( \
             EXISTS ( \
                 SELECT 1 FROM link_graph_space_states state \
                 WHERE state.space_id = space.id \
                   AND state.full_scan_event_id IS NOT NULL \
             ) OR EXISTS ( \
                 SELECT 1 FROM node_link_projections projection \
                 LEFT JOIN background_jobs job ON job.job_id = projection.active_job_id \
                 WHERE projection.space_id = space.id \
                   AND {NODE_REQUEST_PENDING_PREDICATE} \
             ) \
         ) \
         FROM spaces space \
         WHERE space.id = $1 AND space.deleted_at IS NULL"
    )))
    .bind(space_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(map_sqlx_error)
}

pub(super) async fn node_request_pending_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<bool> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS ( \
             SELECT 1 \
             FROM node_link_projections projection \
             LEFT JOIN background_jobs job ON job.job_id = projection.active_job_id \
             WHERE projection.space_id = $1 \
               AND projection.source_node_id = $2 \
               AND {NODE_REQUEST_PENDING_PREDICATE} \
         )"
    )))
    .bind(space_id)
    .bind(node_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(map_sqlx_error)
}
