//! Queue capacity, bounded dispatch, and atomic target-to-job association.

use std::collections::BTreeMap;

use notegate_core::Result;
use notegate_jobs::{JobHistoryContext, JobQueue, JobSpec, NewJob};
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use super::targets::{TargetScope, scope_parameters};
use super::{
    LINK_GRAPH_ACTIVE_JOB_MAX, LINK_GRAPH_PROJECT_BATCH_MAX, LinkGraphProjectNodesJob,
    LinkGraphProjectNodesPayload, LinkGraphProjectSource,
};
use crate::map_sqlx_error;

const LINK_GRAPH_DISPATCH_BATCH_SIZE: usize = 500;
const LINK_GRAPH_DISPATCH_FETCH_LIMIT: i64 = 501;
const LINK_GRAPH_PROJECT_MAX_ATTEMPTS: i32 = 8;
const LINK_GRAPH_DISPATCH_LOCK_SEED: i64 = 0x4e47_4c49_4e4b_0001;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DispatchSummary {
    pub(super) targets: usize,
    pub(super) jobs: usize,
    pub(super) has_more: bool,
    pub(super) backpressured: bool,
}

#[derive(Debug, FromRow)]
struct DispatchCandidateRow {
    space_id: Uuid,
    owner_user_id: Option<Uuid>,
    space_name: Option<String>,
    node_id: Uuid,
    expected_content_sha256: Option<String>,
}

#[derive(Debug)]
struct DispatchSpaceBatch {
    owner_user_id: Option<Uuid>,
    space_name: Option<String>,
    sources: Vec<LinkGraphProjectSource>,
}

pub(super) async fn dispatch_targets_in(
    connection: &mut PgConnection,
    scope: TargetScope<'_>,
) -> Result<DispatchSummary> {
    let (space_id, node_ids) = scope_parameters(scope);
    let capacity_locked: bool = sqlx::query_scalar(
        "SELECT pg_try_advisory_xact_lock(hashtextextended(current_schema(), $1))",
    )
    .bind(LINK_GRAPH_DISPATCH_LOCK_SEED)
    .fetch_one(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    if !capacity_locked {
        return pending_dispatch_summary(connection, space_id, node_ids.as_deref()).await;
    }

    let active_jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM background_jobs \
         WHERE job_kind = $1 AND status IN ('queued', 'running')",
    )
    .bind(LinkGraphProjectNodesJob::KIND)
    .fetch_one(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    let job_capacity =
        usize::try_from(LINK_GRAPH_ACTIVE_JOB_MAX.saturating_sub(active_jobs).max(0))
            .map_err(|_error| notegate_core::Error::internal("link graph job capacity overflow"))?;
    if job_capacity == 0 {
        return pending_dispatch_summary(connection, space_id, node_ids.as_deref()).await;
    }

    let mut rows = sqlx::query_as::<_, DispatchCandidateRow>(
        "SELECT projection.space_id, space.owner_user_id, space.name AS space_name, \
                projection.source_node_id AS node_id, \
                text.content_sha256 AS expected_content_sha256 \
         FROM node_link_projections projection \
         LEFT JOIN spaces space ON space.id = projection.space_id \
         LEFT JOIN nodes node ON node.space_id = projection.space_id \
           AND node.id = projection.source_node_id AND node.kind = 'text' \
           AND node.deleted_at IS NULL \
         LEFT JOIN text_objects text ON text.space_id = node.space_id \
           AND text.node_id = node.id \
         WHERE ($1::uuid IS NULL OR projection.space_id = $1) \
           AND ($2::uuid[] IS NULL OR projection.source_node_id = ANY($2)) \
           AND projection.needs_projection \
           AND projection.active_job_id IS NULL AND projection.failed_at IS NULL \
         ORDER BY projection.space_id, projection.source_node_id \
         LIMIT $3 FOR UPDATE OF projection SKIP LOCKED",
    )
    .bind(space_id)
    .bind(node_ids)
    .bind(LINK_GRAPH_DISPATCH_FETCH_LIMIT)
    .fetch_all(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    let fetched_more = rows.len() > LINK_GRAPH_DISPATCH_BATCH_SIZE;
    rows.truncate(LINK_GRAPH_DISPATCH_BATCH_SIZE);
    let candidate_count = rows.len();

    let mut by_space = BTreeMap::<Uuid, DispatchSpaceBatch>::new();
    for row in rows {
        by_space
            .entry(row.space_id)
            .or_insert_with(|| DispatchSpaceBatch {
                owner_user_id: row.owner_user_id,
                space_name: row.space_name.clone(),
                sources: Vec::new(),
            })
            .sources
            .push(LinkGraphProjectSource {
                node_id: row.node_id,
                expected_content_sha256: row.expected_content_sha256,
            });
    }

    let mut targets = 0;
    let mut jobs = 0;
    'spaces: for (space_id, space_batch) in by_space {
        for batch in space_batch.sources.chunks(LINK_GRAPH_PROJECT_BATCH_MAX) {
            if jobs == job_capacity {
                break 'spaces;
            }
            let payload = LinkGraphProjectNodesPayload {
                space_id,
                sources: batch.to_vec(),
            };
            let mut job = NewJob::<LinkGraphProjectNodesJob>::new(payload)
                .max_attempts(LINK_GRAPH_PROJECT_MAX_ATTEMPTS);
            if let (Some(owner_user_id), Some(space_name)) =
                (space_batch.owner_user_id, space_batch.space_name.as_ref())
            {
                job = job.record_in_history(
                    owner_user_id,
                    Some(
                        JobHistoryContext::new("space")
                            .id(space_id)
                            .label(space_name),
                    ),
                );
            }
            let enqueued = JobQueue::enqueue_in(connection, &job)
                .await
                .map_err(job_error)?;
            sqlx::query(
                "UPDATE node_link_projections \
                 SET active_job_id = $3, active_request_version = request_version \
                 WHERE space_id = $1 AND source_node_id = ANY($2)",
            )
            .bind(space_id)
            .bind(
                batch
                    .iter()
                    .map(|source| source.node_id)
                    .collect::<Vec<_>>(),
            )
            .bind(enqueued.job_id)
            .execute(&mut *connection)
            .await
            .map_err(map_sqlx_error)?;
            targets += batch.len();
            jobs += 1;
        }
    }
    let capacity_exhausted = jobs == job_capacity && (targets < candidate_count || fetched_more);
    Ok(DispatchSummary {
        targets,
        jobs,
        has_more: targets < candidate_count || fetched_more,
        backpressured: capacity_exhausted,
    })
}

async fn pending_dispatch_summary(
    connection: &mut PgConnection,
    space_id: Option<Uuid>,
    node_ids: Option<&[Uuid]>,
) -> Result<DispatchSummary> {
    let node_ids = node_ids.map(<[Uuid]>::to_vec);
    let has_more: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM node_link_projections projection \
             WHERE ($1::uuid IS NULL OR projection.space_id = $1) \
               AND ($2::uuid[] IS NULL OR projection.source_node_id = ANY($2)) \
               AND projection.needs_projection \
               AND projection.active_job_id IS NULL AND projection.failed_at IS NULL \
         )",
    )
    .bind(space_id)
    .bind(node_ids)
    .fetch_one(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    Ok(DispatchSummary {
        has_more,
        backpressured: has_more,
        ..DispatchSummary::default()
    })
}

fn job_error(error: notegate_jobs::JobQueueError) -> notegate_core::Error {
    notegate_core::Error::internal(format!("link graph job queue failed: {error}"))
}
