use std::collections::{BTreeMap, BTreeSet};

use notegate_core::Result;
use notegate_jobs::{JobQueue, JobSpec, NewJob};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

pub const LINK_GRAPH_PROJECT_BATCH_MAX: usize = 50;
pub(crate) const LINK_GRAPH_PROCESSOR_KIND: &str = "link_graph";
const LINK_GRAPH_CHANGE_BATCH_SIZE: usize = 500;
const LINK_GRAPH_CHANGE_FETCH_LIMIT: i64 = 501;
const LINK_GRAPH_CHANGE_SPACE_BATCH_SIZE: i64 = 32;
const LINK_GRAPH_DISPATCH_BATCH_SIZE: usize = 500;
const LINK_GRAPH_DISPATCH_FETCH_LIMIT: i64 = 501;
const LINK_GRAPH_SETTLEMENT_BATCH_SIZE: i64 = 500;
const LINK_GRAPH_PROJECT_MAX_ATTEMPTS: i32 = 8;

pub struct LinkGraphProjectNodesJob;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkGraphProjectNodesPayload {
    pub space_id: Uuid,
    pub node_ids: Vec<Uuid>,
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
        settle_terminal_targets_in(&mut tx, scope, i64::MAX).await?;
        stage_node_ids_in(&mut tx, space_id, node_ids).await?;
        dispatch_targets_in(&mut tx, scope).await?;
        tx.commit().await.map_err(map_sqlx_error)
    }

    pub async fn request_space(&self, space_id: Uuid) -> Result<bool> {
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
            return Ok(false);
        }
        lock_processor_state_in(&mut tx, space_id).await?;
        let scope = TargetScope::Space(space_id);
        let settled =
            settle_terminal_targets_in(&mut tx, scope, LINK_GRAPH_SETTLEMENT_BATCH_SIZE).await?;
        stage_full_space_in(&mut tx, space_id).await?;
        let latest_event_id = latest_event_id(&mut tx, space_id).await?;
        let dispatched = dispatch_targets_in(&mut tx, scope).await?;
        update_processor_state(
            &mut tx,
            space_id,
            latest_event_id,
            settled.has_more || dispatched.has_more,
        )
        .await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(true)
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
            "SELECT node_id, active_request_version AS request_version \
             FROM node_link_projection_targets \
             WHERE active_job_id = $1 AND active_request_version IS NOT NULL \
               AND space_id = $2 AND node_id = ANY($3) \
             ORDER BY node_id",
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
        let space_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT space_id FROM space_change_processor_states \
             WHERE processor_kind = $1 AND processing_state = 'pending' \
               AND available_at <= now() \
             ORDER BY available_at, space_id LIMIT $2",
        )
        .bind(LINK_GRAPH_PROCESSOR_KIND)
        .bind(LINK_GRAPH_CHANGE_SPACE_BATCH_SIZE)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let candidate_count = space_ids.len();
        let mut spaces = 0;
        let mut events = 0;
        let mut targets = 0;
        let mut dispatched_targets = 0;
        let mut jobs = 0;
        let mut has_more = candidate_count == LINK_GRAPH_CHANGE_SPACE_BATCH_SIZE as usize;
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
        }

        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let settled =
            settle_terminal_targets_in(&mut tx, TargetScope::All, LINK_GRAPH_SETTLEMENT_BATCH_SIZE)
                .await?;
        let dispatched = dispatch_targets_in(&mut tx, TargetScope::All).await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        dispatched_targets += dispatched.targets;
        jobs += dispatched.jobs;
        has_more |= settled.has_more || dispatched.has_more;

        if spaces == 0 && settled.failed == 0 && dispatched_targets == 0 {
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
        let state: Option<(i64, bool)> = sqlx::query_as(
            "SELECT last_processed_event_id, requires_full_scan \
             FROM space_change_processor_states \
             WHERE space_id = $1 AND processor_kind = $2 \
               AND processing_state = 'pending' AND available_at <= now() \
             FOR UPDATE",
        )
        .bind(space_id)
        .bind(LINK_GRAPH_PROCESSOR_KIND)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let Some((last_processed_event_id, requires_full_scan)) = state else {
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

        if requires_full_scan {
            let targets = stage_full_space_in(&mut tx, space_id).await?;
            let latest_event_id = latest_event_id(&mut tx, space_id).await?;
            let dispatched = dispatch_targets_in(&mut tx, TargetScope::Space(space_id)).await?;
            update_processor_state(&mut tx, space_id, latest_event_id, dispatched.has_more).await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(Some(CollectedSpace {
                targets,
                dispatched_targets: dispatched.targets,
                jobs: dispatched.jobs,
                has_more: dispatched.has_more,
                ..CollectedSpace::default()
            }));
        }

        let (checkpoint_valid, mut rows) =
            load_event_window(&mut tx, space_id, last_processed_event_id).await?;
        if !checkpoint_valid {
            let targets = stage_full_space_in(&mut tx, space_id).await?;
            let latest_event_id = latest_event_id(&mut tx, space_id).await?;
            let dispatched = dispatch_targets_in(&mut tx, TargetScope::Space(space_id)).await?;
            update_processor_state(&mut tx, space_id, latest_event_id, dispatched.has_more).await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(Some(CollectedSpace {
                targets,
                dispatched_targets: dispatched.targets,
                jobs: dispatched.jobs,
                has_more: dispatched.has_more,
                ..CollectedSpace::default()
            }));
        }

        let has_more = rows.len() > LINK_GRAPH_CHANGE_BATCH_SIZE;
        if has_more {
            rows.pop();
        }
        if rows.is_empty() {
            let dispatched = dispatch_targets_in(&mut tx, TargetScope::Space(space_id)).await?;
            update_processor_state(
                &mut tx,
                space_id,
                last_processed_event_id,
                dispatched.has_more,
            )
            .await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(Some(CollectedSpace {
                dispatched_targets: dispatched.targets,
                jobs: dispatched.jobs,
                has_more: dispatched.has_more,
                ..CollectedSpace::default()
            }));
        }

        let last_event_id = rows
            .last()
            .map_or(last_processed_event_id, |event| event.id);
        let plan = classify_changes(&rows);
        if plan.rebuild {
            let targets = stage_full_space_in(&mut tx, space_id).await?;
            let latest_event_id = latest_event_id(&mut tx, space_id).await?;
            let dispatched = dispatch_targets_in(&mut tx, TargetScope::Space(space_id)).await?;
            update_processor_state(&mut tx, space_id, latest_event_id, dispatched.has_more).await?;
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(Some(CollectedSpace {
                events: rows.len(),
                targets,
                dispatched_targets: dispatched.targets,
                jobs: dispatched.jobs,
                has_more: dispatched.has_more,
            }));
        }

        let targets = stage_incremental_plan(&mut tx, space_id, plan).await?;
        let dispatched = dispatch_targets_in(&mut tx, TargetScope::Space(space_id)).await?;
        let has_more = has_more || dispatched.has_more;
        update_processor_state(&mut tx, space_id, last_event_id, has_more).await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(Some(CollectedSpace {
            events: rows.len(),
            targets,
            dispatched_targets: dispatched.targets,
            jobs: dispatched.jobs,
            has_more,
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DispatchSummary {
    targets: usize,
    jobs: usize,
    has_more: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SettlementSummary {
    failed: usize,
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
    deleted_roots: BTreeSet<Uuid>,
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
            "item.delete" => match event.node_id {
                Some(node_id) => {
                    plan.deleted_roots.insert(node_id);
                }
                None => plan.rebuild = true,
            },
            "item.update" if name_changed(&event.metadata) == Some(false) => {}
            "folder.create" | "text.create" | "file.create" | "item.move" | "item.update"
            | "item.copy" => plan.rebuild = true,
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
) -> Result<(bool, Vec<LinkChangeEventRow>)> {
    let rows = sqlx::query_as::<_, CheckpointedLinkChangeEventRow>(
        "WITH checkpoint AS ( \
             SELECT ($2 = 0 OR EXISTS ( \
                 SELECT 1 FROM file_change_events \
                 WHERE space_id = $1 AND id = $2 \
             )) AS checkpoint_valid \
         ), events AS ( \
             SELECT id, node_id, op_type, metadata \
             FROM file_change_events \
             WHERE space_id = $1 AND id > $2 \
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

async fn stage_incremental_plan(
    connection: &mut PgConnection,
    space_id: Uuid,
    plan: LinkChangePlan,
) -> Result<usize> {
    debug_assert!(!plan.rebuild);

    let mut node_ids = plan.node_ids;
    if !plan.deleted_roots.is_empty() {
        let deleted_roots = plan.deleted_roots.into_iter().collect::<Vec<_>>();
        let deleted_ids: Vec<Uuid> = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
                 SELECT id FROM nodes \
                 WHERE space_id = $1 AND id = ANY($2) AND deleted_at IS NOT NULL \
                 UNION \
                 SELECT child.id FROM nodes child \
                 JOIN subtree parent ON child.parent_id = parent.id \
                 WHERE child.space_id = $1 AND child.deleted_at IS NOT NULL \
             ) \
             SELECT id FROM subtree ORDER BY id",
        )
        .bind(space_id)
        .bind(deleted_roots)
        .fetch_all(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
        node_ids.extend(deleted_ids);
    }

    let node_ids = node_ids.into_iter().collect::<Vec<_>>();
    stage_node_ids_in(connection, space_id, &node_ids).await
}

async fn stage_node_ids_in(
    connection: &mut PgConnection,
    space_id: Uuid,
    node_ids: &[Uuid],
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
             JOIN nodes node ON node.id = input.node_id AND node.space_id = $1 \
         ) \
         INSERT INTO node_link_projection_targets (space_id, node_id, request_version) \
         SELECT $1, candidates.node_id, \
                nextval('node_link_projection_request_version_seq') \
         FROM candidates \
         ON CONFLICT (space_id, node_id) DO UPDATE \
         SET request_version = EXCLUDED.request_version, \
             failure_code = NULL, failed_at = NULL, updated_at = now()",
    )
    .bind(space_id)
    .bind(node_ids)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();
    usize::try_from(affected)
        .map_err(|_error| notegate_core::Error::internal("link target count overflow"))
}

async fn stage_full_space_in(connection: &mut PgConnection, space_id: Uuid) -> Result<usize> {
    let affected = sqlx::query(
        "WITH candidates AS ( \
             SELECT node.id AS node_id \
             FROM nodes node \
             WHERE node.space_id = $1 AND node.kind = 'text' \
               AND node.deleted_at IS NULL \
             UNION \
             SELECT state.source_node_id AS node_id \
             FROM node_link_source_states state \
             WHERE state.space_id = $1 \
             UNION \
             SELECT reference.source_node_id AS node_id \
             FROM node_link_refs reference \
             WHERE reference.space_id = $1 \
         ) \
         INSERT INTO node_link_projection_targets (space_id, node_id, request_version) \
         SELECT $1, node_id, \
                nextval('node_link_projection_request_version_seq') \
         FROM candidates \
         ON CONFLICT (space_id, node_id) DO UPDATE \
         SET request_version = EXCLUDED.request_version, \
             failure_code = NULL, failed_at = NULL, updated_at = now()",
    )
    .bind(space_id)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();
    usize::try_from(affected)
        .map_err(|_error| notegate_core::Error::internal("link target count overflow"))
}

async fn settle_terminal_targets_in(
    connection: &mut PgConnection,
    scope: TargetScope<'_>,
    limit: i64,
) -> Result<SettlementSummary> {
    let (space_id, node_ids) = scope_parameters(scope);
    let (processed, failed): (i64, i64) = sqlx::query_as(
        "WITH candidates AS ( \
             SELECT target.space_id, target.node_id, target.request_version, \
                    target.active_request_version, job.status, \
                    job.last_error_code, job.completed_at \
             FROM node_link_projection_targets target \
             JOIN background_jobs job ON job.job_id = target.active_job_id \
             WHERE ($1::uuid IS NULL OR target.space_id = $1) \
               AND ($2::uuid[] IS NULL OR target.node_id = ANY($2)) \
               AND job.status IN ('succeeded', 'dead') \
             ORDER BY target.space_id, target.node_id \
             LIMIT $3 FOR UPDATE OF target SKIP LOCKED \
         ), updated AS ( \
             UPDATE node_link_projection_targets target \
             SET active_job_id = NULL, active_request_version = NULL, \
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
                 END, \
                 updated_at = now() \
             FROM candidates candidate \
             WHERE target.space_id = candidate.space_id \
               AND target.node_id = candidate.node_id \
             RETURNING target.failure_code IS NOT NULL AS failed \
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
    let mut rows = sqlx::query_as::<_, DispatchCandidateRow>(
        "SELECT target.space_id, target.node_id \
         FROM node_link_projection_targets target \
         WHERE ($1::uuid IS NULL OR target.space_id = $1) \
           AND ($2::uuid[] IS NULL OR target.node_id = ANY($2)) \
           AND target.active_job_id IS NULL AND target.failed_at IS NULL \
         ORDER BY target.space_id, target.node_id \
         LIMIT $3 FOR UPDATE OF target SKIP LOCKED",
    )
    .bind(space_id)
    .bind(node_ids)
    .bind(LINK_GRAPH_DISPATCH_FETCH_LIMIT)
    .fetch_all(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    let has_more = rows.len() > LINK_GRAPH_DISPATCH_BATCH_SIZE;
    rows.truncate(LINK_GRAPH_DISPATCH_BATCH_SIZE);

    let mut by_space = BTreeMap::<Uuid, Vec<Uuid>>::new();
    for row in rows {
        by_space.entry(row.space_id).or_default().push(row.node_id);
    }

    let mut targets = 0;
    let mut jobs = 0;
    for (space_id, node_ids) in by_space {
        for batch in node_ids.chunks(LINK_GRAPH_PROJECT_BATCH_MAX) {
            let payload = LinkGraphProjectNodesPayload {
                space_id,
                node_ids: batch.to_vec(),
            };
            let enqueued = JobQueue::enqueue_in(
                connection,
                &NewJob::<LinkGraphProjectNodesJob>::new(payload)
                    .max_attempts(LINK_GRAPH_PROJECT_MAX_ATTEMPTS),
            )
            .await
            .map_err(job_error)?;
            sqlx::query(
                "UPDATE node_link_projection_targets \
                 SET active_job_id = $3, active_request_version = request_version, \
                     updated_at = now() \
                 WHERE space_id = $1 AND node_id = ANY($2)",
            )
            .bind(space_id)
            .bind(batch)
            .bind(enqueued.job_id)
            .execute(&mut *connection)
            .await
            .map_err(map_sqlx_error)?;
            targets += batch.len();
            jobs += 1;
        }
    }
    Ok(DispatchSummary {
        targets,
        jobs,
        has_more,
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

async fn lock_processor_state_in(connection: &mut PgConnection, space_id: Uuid) -> Result<()> {
    sqlx::query(
        "INSERT INTO space_change_processor_states (space_id, processor_kind) \
         VALUES ($1, $2) ON CONFLICT (space_id, processor_kind) DO NOTHING",
    )
    .bind(space_id)
    .bind(LINK_GRAPH_PROCESSOR_KIND)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    sqlx::query(
        "SELECT 1 FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = $2 FOR UPDATE",
    )
    .bind(space_id)
    .bind(LINK_GRAPH_PROCESSOR_KIND)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

async fn update_processor_state(
    connection: &mut PgConnection,
    space_id: Uuid,
    last_processed_event_id: i64,
    pending: bool,
) -> Result<()> {
    sqlx::query(
        "UPDATE space_change_processor_states \
         SET last_processed_event_id = $3, \
             processing_state = CASE WHEN $4 THEN 'pending' ELSE 'idle' END, \
             available_at = CASE WHEN $4 THEN now() ELSE NULL END, \
             requires_full_scan = false, updated_at = now() \
         WHERE space_id = $1 AND processor_kind = $2",
    )
    .bind(space_id)
    .bind(LINK_GRAPH_PROCESSOR_KIND)
    .bind(last_processed_event_id)
    .bind(pending)
    .execute(&mut *connection)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

pub(crate) async fn cleanup_space_in(connection: &mut PgConnection, space_id: Uuid) -> Result<()> {
    sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1")
        .bind(space_id)
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    sqlx::query("DELETE FROM node_link_source_states WHERE space_id = $1")
        .bind(space_id)
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    sqlx::query("DELETE FROM node_link_projection_targets WHERE space_id = $1")
        .bind(space_id)
        .execute(&mut *connection)
        .await
        .map_err(map_sqlx_error)?;
    sqlx::query(
        "DELETE FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = $2",
    )
    .bind(space_id)
    .bind(LINK_GRAPH_PROCESSOR_KIND)
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
        assert!(plan.deleted_roots.is_empty());
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
        assert!(plan.deleted_roots.is_empty());
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
    fn deletion_keeps_the_root_for_retained_subtree_expansion() {
        let root = Uuid::new_v4();
        let plan = classify_changes(&[event("item.delete", Some(root), json!({}))]);

        assert!(!plan.rebuild);
        assert_eq!(plan.deleted_roots, BTreeSet::from([root]));
    }
}
