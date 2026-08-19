use std::collections::{BTreeMap, BTreeSet};

use notegate_core::Result;
use notegate_jobs::{JobQueue, JobSpec, NewJob};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

pub const LINK_GRAPH_PROJECT_BATCH_MAX: usize = 50;
pub const LINK_GRAPH_ACTIVE_JOB_MAX: i64 = 1_000;
const LINK_GRAPH_CHANGE_BATCH_SIZE: usize = 500;
const LINK_GRAPH_CHANGE_FETCH_LIMIT: i64 = 501;
const LINK_GRAPH_CHANGE_SPACE_BATCH_SIZE: usize = 32;
const LINK_GRAPH_CHANGE_SPACE_FETCH_LIMIT: i64 = 33;
const LINK_GRAPH_FULL_SCAN_BATCH_SIZE: usize = 500;
const LINK_GRAPH_FULL_SCAN_FETCH_LIMIT: i64 = 501;
const LINK_GRAPH_DISPATCH_BATCH_SIZE: usize = 500;
const LINK_GRAPH_DISPATCH_FETCH_LIMIT: i64 = 501;
const LINK_GRAPH_SETTLEMENT_BATCH_SIZE: i64 = 500;
const LINK_GRAPH_PROJECT_MAX_ATTEMPTS: i32 = 8;
const LINK_GRAPH_DISPATCH_LOCK_SEED: i64 = 0x4e47_4c49_4e4b_0001;

pub struct LinkGraphProjectNodesJob;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkGraphProjectNodesPayload {
    pub space_id: Uuid,
    pub sources: Vec<LinkGraphProjectSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkGraphProjectSource {
    pub node_id: Uuid,
    pub expected_content_sha256: Option<String>,
}

impl JobSpec for LinkGraphProjectNodesJob {
    const KIND: &'static str = "link_graph_project_nodes";
    type Payload = LinkGraphProjectNodesPayload;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkGraphProjectionTarget {
    pub node_id: Uuid,
    pub request_version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkGraphChangeCollection {
    Idle,
    Collected {
        spaces: usize,
        events: usize,
        staged_targets: usize,
        failed_targets: usize,
        dispatched_targets: usize,
        jobs: usize,
        has_more: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkGraphSpaceRequestOutcome {
    Requested,
    AlreadyPending,
    NotFound,
}

#[derive(Debug, Clone)]
pub struct LinkGraphWorkRepo {
    pool: PgPool,
}

impl LinkGraphWorkRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn request_nodes(&self, space_id: Uuid, node_ids: &[Uuid]) -> Result<()> {
        validate_node_batch(node_ids)?;
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let scope = TargetScope::Nodes { space_id, node_ids };
        stage_node_ids_in(&mut tx, space_id, node_ids, true).await?;
        dispatch_targets_in(&mut tx, scope).await?;
        tx.commit().await.map_err(map_sqlx_error)
    }

    pub async fn space_pending(&self, space_id: Uuid) -> Result<Option<bool>> {
        sqlx::query_scalar(
            "SELECT ( \
                 EXISTS ( \
                     SELECT 1 FROM link_graph_space_states state \
                     WHERE state.space_id = space.id AND state.available_at IS NOT NULL \
                 ) OR EXISTS ( \
                     SELECT 1 FROM node_link_projections projection \
                     WHERE projection.space_id = space.id AND projection.needs_projection \
                 ) \
             ) \
             FROM spaces space \
             WHERE space.id = $1 AND space.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn request_space(&self, space_id: Uuid) -> Result<LinkGraphSpaceRequestOutcome> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let live: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM spaces WHERE id = $1 AND deleted_at IS NULL)",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if !live {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphSpaceRequestOutcome::NotFound);
        }
        lock_space_state_in(&mut tx, space_id).await?;
        if space_pending_in(&mut tx, space_id).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphSpaceRequestOutcome::AlreadyPending);
        }
        let full_scan_event_id = start_full_scan_state(&mut tx, space_id).await?;
        run_full_scan_pass(&mut tx, space_id, full_scan_event_id, None).await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(LinkGraphSpaceRequestOutcome::Requested)
    }

    pub async fn dispatch_ready_nodes(&self, space_id: Uuid, node_ids: &[Uuid]) -> Result<()> {
        validate_node_batch(node_ids)?;
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        dispatch_targets_in(&mut tx, TargetScope::Nodes { space_id, node_ids }).await?;
        tx.commit().await.map_err(map_sqlx_error)
    }

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

    pub async fn collect_changes(&self) -> Result<LinkGraphChangeCollection> {
        let mut space_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT space_id FROM link_graph_space_states \
             WHERE available_at IS NOT NULL AND ( \
                 incremental_event_id IS NOT NULL \
                 OR full_scan_event_id IS NOT NULL \
                 OR available_at <= now() \
             ) \
             ORDER BY (incremental_event_id IS NOT NULL \
                       OR full_scan_event_id IS NOT NULL) DESC, \
                      available_at, space_id \
             LIMIT $1",
        )
        .bind(LINK_GRAPH_CHANGE_SPACE_FETCH_LIMIT)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let mut has_more = space_ids.len() > LINK_GRAPH_CHANGE_SPACE_BATCH_SIZE;
        space_ids.truncate(LINK_GRAPH_CHANGE_SPACE_BATCH_SIZE);
        let mut spaces = 0;
        let mut events = 0;
        let mut targets = 0;
        let mut dispatched_targets = 0;
        let mut jobs = 0;
        let mut backpressured = false;
        for space_id in space_ids {
            let Some(collected) = self.collect_space_changes(space_id).await? else {
                continue;
            };
            spaces += 1;
            events += collected.events;
            targets += collected.targets;
            dispatched_targets += collected.dispatched_targets;
            jobs += collected.jobs;
            has_more |= collected.has_more;
            backpressured |= collected.backpressured;
        }

        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let settled =
            settle_terminal_targets_in(&mut tx, TargetScope::All, LINK_GRAPH_SETTLEMENT_BATCH_SIZE)
                .await?;
        let dispatched = dispatch_targets_in(&mut tx, TargetScope::All).await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        dispatched_targets += dispatched.targets;
        jobs += dispatched.jobs;
        has_more |= settled.has_more || (dispatched.has_more && !dispatched.backpressured);
        backpressured |= dispatched.backpressured;

        if spaces == 0 && settled.failed == 0 && dispatched_targets == 0 && !backpressured {
            return Ok(LinkGraphChangeCollection::Idle);
        }
        Ok(LinkGraphChangeCollection::Collected {
            spaces,
            events,
            staged_targets: targets,
            failed_targets: settled.failed,
            dispatched_targets,
            jobs,
            has_more,
        })
    }

    async fn collect_space_changes(&self, space_id: Uuid) -> Result<Option<CollectedSpace>> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let state = sqlx::query_as::<_, LinkGraphSpaceStateRow>(
            "SELECT last_processed_event_id, pending_since_event_id, incremental_event_id, \
                    full_scan_event_id, full_scan_after_node_id \
             FROM link_graph_space_states \
             WHERE space_id = $1 AND available_at IS NOT NULL AND ( \
                 incremental_event_id IS NOT NULL \
                 OR full_scan_event_id IS NOT NULL \
                 OR available_at <= now() \
             ) \
             FOR UPDATE",
        )
        .bind(space_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let Some(state) = state else {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(None);
        };

        let live: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM spaces WHERE id = $1 AND deleted_at IS NULL)",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if !live {
            cleanup_space_in(&mut tx, space_id).await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(Some(CollectedSpace::default()));
        }

        if let Some(full_scan_event_id) = state.full_scan_event_id {
            let collected = run_full_scan_pass(
                &mut tx,
                space_id,
                full_scan_event_id,
                state.full_scan_after_node_id,
            )
            .await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(Some(collected));
        }

        let event_window_id = match state.incremental_event_id {
            Some(event_id) => event_id,
            None => latest_event_id(&mut tx, space_id).await?,
        };
        let (checkpoint_valid, mut rows) = load_event_window(
            &mut tx,
            space_id,
            state.last_processed_event_id,
            state.pending_since_event_id,
            event_window_id,
        )
        .await?;
        if !checkpoint_valid {
            let collected = run_full_scan_pass(&mut tx, space_id, event_window_id, None).await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(Some(collected));
        }

        let has_more = rows.len() > LINK_GRAPH_CHANGE_BATCH_SIZE;
        if has_more {
            rows.pop();
        }
        if rows.is_empty() {
            let dispatched = dispatch_targets_in(&mut tx, TargetScope::Space(space_id)).await?;
            let events_after_window = latest_event_id(&mut tx, space_id).await? > event_window_id;
            let pending = dispatched.has_more || events_after_window;
            update_space_state(
                &mut tx,
                space_id,
                state.last_processed_event_id,
                pending,
                dispatched.has_more.then_some(event_window_id),
            )
            .await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(Some(CollectedSpace {
                dispatched_targets: dispatched.targets,
                jobs: dispatched.jobs,
                has_more: dispatched.has_more && !dispatched.backpressured,
                backpressured: dispatched.backpressured,
                ..CollectedSpace::default()
            }));
        }

        let last_event_id = rows
            .last()
            .map_or(state.last_processed_event_id, |event| event.id);
        let plan = classify_changes(&rows);
        if plan.rebuild {
            let mut collected =
                run_full_scan_pass(&mut tx, space_id, event_window_id, None).await?;
            collected.events = rows.len();
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(Some(collected));
        }

        let node_ids = plan.node_ids.into_iter().collect::<Vec<_>>();
        let targets = stage_node_ids_in(&mut tx, space_id, &node_ids, false).await?;
        let dispatched = dispatch_targets_in(&mut tx, TargetScope::Space(space_id)).await?;
        let events_after_window = latest_event_id(&mut tx, space_id).await? > event_window_id;
        let has_immediate_work = has_more || (dispatched.has_more && !dispatched.backpressured);
        let pending = has_more || dispatched.has_more || events_after_window;
        update_space_state(
            &mut tx,
            space_id,
            last_event_id,
            pending,
            (has_more || dispatched.has_more).then_some(event_window_id),
        )
        .await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(Some(CollectedSpace {
            events: rows.len(),
            targets,
            dispatched_targets: dispatched.targets,
            jobs: dispatched.jobs,
            has_more: has_immediate_work,
            backpressured: dispatched.backpressured,
        }))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CollectedSpace {
    events: usize,
    targets: usize,
    dispatched_targets: usize,
    jobs: usize,
    has_more: bool,
    backpressured: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DispatchSummary {
    targets: usize,
    jobs: usize,
    has_more: bool,
    backpressured: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SettlementSummary {
    failed: usize,
    has_more: bool,
}

#[derive(Debug, FromRow)]
struct LinkGraphSpaceStateRow {
    last_processed_event_id: i64,
    pending_since_event_id: Option<i64>,
    incremental_event_id: Option<i64>,
    full_scan_event_id: Option<i64>,
    full_scan_after_node_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FullScanBatch {
    targets: usize,
    last_node_id: Option<Uuid>,
    has_more: bool,
}

#[derive(Debug, Clone, Copy)]
enum TargetScope<'a> {
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

#[derive(Debug, FromRow)]
struct DispatchCandidateRow {
    space_id: Uuid,
    node_id: Uuid,
    expected_content_sha256: Option<String>,
}

#[derive(Debug, FromRow)]
struct CheckpointedLinkChangeEventRow {
    checkpoint_valid: bool,
    id: Option<i64>,
    node_id: Option<Uuid>,
    op_type: Option<String>,
    metadata: Option<Value>,
}

#[derive(Debug)]
struct LinkChangeEventRow {
    id: i64,
    node_id: Option<Uuid>,
    op_type: String,
    metadata: Value,
}

#[derive(Debug, Default)]
struct LinkChangePlan {
    rebuild: bool,
    node_ids: BTreeSet<Uuid>,
}

fn classify_changes(events: &[LinkChangeEventRow]) -> LinkChangePlan {
    let mut plan = LinkChangePlan::default();
    for event in events {
        match event.op_type.as_str() {
            "text.write" | "text.append" | "text.patch" | "text.edit" => match event.node_id {
                Some(node_id) => {
                    plan.node_ids.insert(node_id);
                }
                None => plan.rebuild = true,
            },
            "item.update" if name_changed(&event.metadata) == Some(false) => {}
            "folder.create" | "text.create" | "file.create" | "item.move" | "item.update"
            | "item.copy" | "item.delete" => plan.rebuild = true,
            _ => plan.rebuild = true,
        }
    }
    plan
}

fn name_changed(metadata: &Value) -> Option<bool> {
    metadata.get("name_changed").and_then(Value::as_bool)
}

async fn load_event_window(
    connection: &mut PgConnection,
    space_id: Uuid,
    last_processed_event_id: i64,
    pending_since_event_id: Option<i64>,
    event_window_id: i64,
) -> Result<(bool, Vec<LinkChangeEventRow>)> {
    let rows = sqlx::query_as::<_, CheckpointedLinkChangeEventRow>(
        "WITH checkpoint AS ( \
             SELECT ( \
                 ($2 = 0 OR EXISTS ( \
                     SELECT 1 FROM file_change_events \
                     WHERE space_id = $1 AND id = $2 \
                 )) AND ( \
                     $4::bigint IS NULL OR $4 <= $2 OR EXISTS ( \
                     SELECT 1 FROM file_change_events \
                     WHERE space_id = $1 AND id = $4 \
                     ) \
                 ) \
             ) AS checkpoint_valid \
         ), events AS ( \
             SELECT id, node_id, op_type, metadata \
             FROM file_change_events \
             WHERE space_id = $1 AND id > $2 AND id <= $5 \
             ORDER BY id LIMIT $3 \
         ) \
         SELECT checkpoint.checkpoint_valid, events.id, events.node_id, \
                events.op_type, events.metadata \
         FROM checkpoint LEFT JOIN events ON true \
         ORDER BY events.id",
    )
    .bind(space_id)
    .bind(last_processed_event_id)
    .bind(LINK_GRAPH_CHANGE_FETCH_LIMIT)
    .bind(pending_since_event_id)
    .bind(event_window_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    let checkpoint_valid = rows.first().is_none_or(|row| row.checkpoint_valid);
    let events = rows
        .into_iter()
        .filter_map(|row| {
            Some(LinkChangeEventRow {
                id: row.id?,
                node_id: row.node_id,
                op_type: row.op_type?,
                metadata: row.metadata?,
            })
        })
        .collect();
    Ok((checkpoint_valid, events))
}

async fn stage_node_ids_in(
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

async fn stage_full_space_batch_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    after_node_id: Option<Uuid>,
) -> Result<FullScanBatch> {
    const INITIAL_PAGE_SQL: &str = "WITH candidates AS ( \
             (SELECT node.id AS node_id \
              FROM nodes node \
              WHERE node.space_id = $1 AND node.kind = 'text' \
                AND node.deleted_at IS NULL \
              ORDER BY node.id LIMIT $2) \
             UNION \
             (SELECT projection.source_node_id AS node_id \
              FROM node_link_projections projection \
              WHERE projection.space_id = $1 \
              ORDER BY projection.source_node_id LIMIT $2) \
         ) \
         SELECT node_id FROM candidates ORDER BY node_id LIMIT $2";
    const CONTINUATION_PAGE_SQL: &str = "WITH candidates AS ( \
             (SELECT node.id AS node_id \
              FROM nodes node \
             WHERE node.space_id = $1 AND node.kind = 'text' \
                AND node.deleted_at IS NULL AND node.id > $2 \
              ORDER BY node.id LIMIT $3) \
             UNION \
             (SELECT projection.source_node_id AS node_id \
              FROM node_link_projections projection \
              WHERE projection.space_id = $1 AND projection.source_node_id > $2 \
              ORDER BY projection.source_node_id LIMIT $3) \
         ) \
         SELECT node_id FROM candidates ORDER BY node_id LIMIT $3";

    let mut query = sqlx::query_scalar::<_, Uuid>(match after_node_id {
        Some(_after_node_id) => CONTINUATION_PAGE_SQL,
        None => INITIAL_PAGE_SQL,
    })
    .bind(space_id);
    if let Some(after_node_id) = after_node_id {
        query = query.bind(after_node_id);
    }
    let mut node_ids = query
        .bind(LINK_GRAPH_FULL_SCAN_FETCH_LIMIT)
        .fetch_all(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    let has_more = node_ids.len() > LINK_GRAPH_FULL_SCAN_BATCH_SIZE;
    node_ids.truncate(LINK_GRAPH_FULL_SCAN_BATCH_SIZE);
    let last_node_id = node_ids.last().copied();
    let targets = stage_node_ids_in(connection, space_id, &node_ids, true).await?;
    Ok(FullScanBatch {
        targets,
        last_node_id,
        has_more,
    })
}

async fn run_full_scan_pass(
    connection: &mut PgConnection,
    space_id: Uuid,
    full_scan_event_id: i64,
    after_node_id: Option<Uuid>,
) -> Result<CollectedSpace> {
    let batch = stage_full_space_batch_in(connection, space_id, after_node_id).await?;
    let dispatched = dispatch_targets_in(connection, TargetScope::Space(space_id)).await?;

    let events_after_scan = if batch.has_more {
        false
    } else {
        latest_event_id(connection, space_id).await? > full_scan_event_id
    };
    let has_immediate_work = batch.has_more || (dispatched.has_more && !dispatched.backpressured);
    let pending = batch.has_more || dispatched.has_more || events_after_scan;
    if batch.has_more {
        let last_node_id = batch.last_node_id.ok_or_else(|| {
            notegate_core::Error::internal("full link scan has no continuation node")
        })?;
        update_full_scan_progress(connection, space_id, full_scan_event_id, last_node_id).await?;
    } else {
        update_space_state(
            connection,
            space_id,
            full_scan_event_id,
            pending,
            dispatched.has_more.then_some(full_scan_event_id),
        )
        .await?;
    }

    Ok(CollectedSpace {
        targets: batch.targets,
        dispatched_targets: dispatched.targets,
        jobs: dispatched.jobs,
        has_more: has_immediate_work,
        backpressured: dispatched.backpressured,
        ..CollectedSpace::default()
    })
}

async fn settle_terminal_targets_in(
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

async fn dispatch_targets_in(
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
        "SELECT projection.space_id, projection.source_node_id AS node_id, \
                text.content_sha256 AS expected_content_sha256 \
         FROM node_link_projections projection \
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

    let mut by_space = BTreeMap::<Uuid, Vec<LinkGraphProjectSource>>::new();
    for row in rows {
        by_space
            .entry(row.space_id)
            .or_default()
            .push(LinkGraphProjectSource {
                node_id: row.node_id,
                expected_content_sha256: row.expected_content_sha256,
            });
    }

    let mut targets = 0;
    let mut jobs = 0;
    'spaces: for (space_id, sources) in by_space {
        for batch in sources.chunks(LINK_GRAPH_PROJECT_BATCH_MAX) {
            if jobs == job_capacity {
                break 'spaces;
            }
            let payload = LinkGraphProjectNodesPayload {
                space_id,
                sources: batch.to_vec(),
            };
            let enqueued = JobQueue::enqueue_in(
                connection,
                &NewJob::<LinkGraphProjectNodesJob>::new(payload)
                    .max_attempts(LINK_GRAPH_PROJECT_MAX_ATTEMPTS),
            )
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

fn scope_parameters(scope: TargetScope<'_>) -> (Option<Uuid>, Option<Vec<Uuid>>) {
    match scope {
        TargetScope::All => (None, None),
        TargetScope::Space(space_id) => (Some(space_id), None),
        TargetScope::Nodes { space_id, node_ids } => (Some(space_id), Some(node_ids.to_vec())),
    }
}

fn validate_node_batch(node_ids: &[Uuid]) -> Result<()> {
    if node_ids.is_empty() || node_ids.len() > LINK_GRAPH_PROJECT_BATCH_MAX {
        return Err(notegate_core::Error::validation(format!(
            "link graph batch must contain between 1 and {LINK_GRAPH_PROJECT_BATCH_MAX} node ids"
        )));
    }
    Ok(())
}

async fn latest_event_id(connection: &mut PgConnection, space_id: Uuid) -> Result<i64> {
    sqlx::query_scalar("SELECT COALESCE(max(id), 0) FROM file_change_events WHERE space_id = $1")
        .bind(space_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(map_sqlx_error)
}

async fn lock_space_state_in(connection: &mut PgConnection, space_id: Uuid) -> Result<()> {
    sqlx::query(
        "INSERT INTO link_graph_space_states (space_id) \
         VALUES ($1) ON CONFLICT (space_id) DO NOTHING",
    )
    .bind(space_id)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    sqlx::query("SELECT 1 FROM link_graph_space_states WHERE space_id = $1 FOR UPDATE")
        .bind(space_id)
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    Ok(())
}

async fn space_pending_in(connection: &mut PgConnection, space_id: Uuid) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT ( \
             EXISTS ( \
                 SELECT 1 FROM link_graph_space_states state \
                 WHERE state.space_id = $1 AND state.available_at IS NOT NULL \
             ) OR EXISTS ( \
                 SELECT 1 FROM node_link_projections projection \
                 WHERE projection.space_id = $1 AND projection.needs_projection \
             ) \
         )",
    )
    .bind(space_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(map_sqlx_error)
}

async fn start_full_scan_state(connection: &mut PgConnection, space_id: Uuid) -> Result<i64> {
    let full_scan_event_id = latest_event_id(connection, space_id).await?;
    sqlx::query(
        "UPDATE link_graph_space_states \
         SET last_processed_event_id = LEAST(last_processed_event_id, $2), \
             available_at = now(), pending_since_event_id = NULL, \
             incremental_event_id = NULL, full_scan_event_id = $2, \
             full_scan_after_node_id = NULL \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .bind(full_scan_event_id)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    Ok(full_scan_event_id)
}

async fn update_full_scan_progress(
    connection: &mut PgConnection,
    space_id: Uuid,
    full_scan_event_id: i64,
    after_node_id: Uuid,
) -> Result<()> {
    sqlx::query(
        "UPDATE link_graph_space_states \
         SET last_processed_event_id = LEAST(last_processed_event_id, $2), \
             available_at = COALESCE(available_at, now()), incremental_event_id = NULL, \
             full_scan_event_id = $2, full_scan_after_node_id = $3 \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .bind(full_scan_event_id)
    .bind(after_node_id)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

async fn update_space_state(
    connection: &mut PgConnection,
    space_id: Uuid,
    last_processed_event_id: i64,
    pending: bool,
    incremental_event_id: Option<i64>,
) -> Result<()> {
    sqlx::query(
        "UPDATE link_graph_space_states \
         SET last_processed_event_id = $2, \
             available_at = CASE \
                 WHEN NOT $3 THEN NULL \
                 ELSE COALESCE(available_at, now()) \
             END, \
             pending_since_event_id = CASE \
                 WHEN $3 AND pending_since_event_id > COALESCE($4, $2) \
                     THEN pending_since_event_id \
                 ELSE NULL \
             END, \
             incremental_event_id = $4, full_scan_event_id = NULL, \
             full_scan_after_node_id = NULL \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .bind(last_processed_event_id)
    .bind(pending)
    .bind(incremental_event_id)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

async fn cleanup_space_in(connection: &mut PgConnection, space_id: Uuid) -> Result<()> {
    sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1")
        .bind(space_id)
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    sqlx::query("DELETE FROM node_link_projections WHERE space_id = $1")
        .bind(space_id)
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    sqlx::query("DELETE FROM link_graph_space_states WHERE space_id = $1")
        .bind(space_id)
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    Ok(())
}

fn job_error(error: notegate_jobs::JobQueueError) -> notegate_core::Error {
    notegate_core::Error::internal(format!("link graph job queue failed: {error}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(op_type: &str, node_id: Option<Uuid>, metadata: Value) -> LinkChangeEventRow {
        LinkChangeEventRow {
            id: 1,
            node_id,
            op_type: op_type.to_owned(),
            metadata,
        }
    }

    #[test]
    fn content_changes_coalesce_by_source() {
        let node_id = Uuid::new_v4();
        let plan = classify_changes(&[
            event("text.write", Some(node_id), json!({})),
            event("text.patch", Some(node_id), json!({})),
        ]);

        assert!(!plan.rebuild);
        assert_eq!(plan.node_ids, BTreeSet::from([node_id]));
    }

    #[test]
    fn non_path_updates_do_not_schedule_graph_work() {
        let plan = classify_changes(&[event(
            "item.update",
            Some(Uuid::new_v4()),
            json!({"name_changed": false, "sort_order_changed": true}),
        )]);

        assert!(!plan.rebuild);
        assert!(plan.node_ids.is_empty());
    }

    #[test]
    fn malformed_item_update_falls_back_to_rebuild() {
        let plan = classify_changes(&[event(
            "item.update",
            Some(Uuid::new_v4()),
            json!({"sort_order_changed": true}),
        )]);

        assert!(plan.rebuild);
    }

    #[test]
    fn topology_changes_collapse_to_one_rebuild() {
        let plan = classify_changes(&[
            event("item.move", Some(Uuid::new_v4()), json!({})),
            event("folder.create", Some(Uuid::new_v4()), json!({})),
        ]);

        assert!(plan.rebuild);
    }

    #[test]
    fn deletion_uses_the_bounded_full_scan_path() {
        let plan = classify_changes(&[event(
            "item.delete",
            Some(Uuid::new_v4()),
            json!({"deleted_nodes": 1}),
        )]);

        assert!(plan.rebuild);
        assert!(plan.node_ids.is_empty());
    }
}
