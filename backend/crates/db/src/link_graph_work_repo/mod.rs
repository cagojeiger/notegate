//! Public link-graph work contract and manual request orchestration.
//!
//! Private modules share the existing transaction connection. Moving a step to
//! another module must not split checkpoint, target, and queue publication.

mod collection;
mod dispatch;
mod targets;

use notegate_core::Result;
use notegate_jobs::JobSpec;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::map_sqlx_error;
use collection::{lock_space_state_in, run_full_scan_pass, start_full_scan_state};
use dispatch::dispatch_targets_in;
use targets::{
    TargetScope, node_request_pending_in, space_pending_in, stage_node_ids_in, validate_node_batch,
};

pub const LINK_GRAPH_PROJECT_BATCH_MAX: usize = 50;
pub const LINK_GRAPH_ACTIVE_JOB_MAX: i64 = 1_000;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkGraphNodeRequestOutcome {
    Requested,
    AlreadyPending,
}

#[derive(Debug, Clone)]
pub struct LinkGraphWorkRepo {
    pool: PgPool,
}

impl LinkGraphWorkRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn request_node(
        &self,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<LinkGraphNodeRequestOutcome> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        lock_space_state_in(&mut tx, space_id).await?;
        if node_request_pending_in(&mut tx, space_id, node_id).await? {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(LinkGraphNodeRequestOutcome::AlreadyPending);
        }
        stage_node_ids_in(&mut tx, space_id, &[node_id], true).await?;
        dispatch_targets_in(
            &mut tx,
            TargetScope::Nodes {
                space_id,
                node_ids: &[node_id],
            },
        )
        .await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(LinkGraphNodeRequestOutcome::Requested)
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
        let mut connection = self.pool.acquire().await.map_err(map_sqlx_error)?;
        space_pending_in(&mut connection, space_id).await
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
        if space_pending_in(&mut tx, space_id).await? == Some(true) {
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
}

/// Restore publishes rebuild intent in the same transaction as making the Space live.
pub(crate) async fn schedule_space_rebuild_in(
    connection: &mut sqlx::PgConnection,
    space_id: Uuid,
) -> Result<()> {
    lock_space_state_in(connection, space_id).await?;
    start_full_scan_state(connection, space_id).await?;
    Ok(())
}
