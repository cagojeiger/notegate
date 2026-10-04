//! Hard purge for soft-deleted spaces and nodes.
//!
//! Cross-process scheduling is owned by the reconciliation runtime. This repo
//! runs bounded resource, identity, and history transactions sequentially.

mod history;
mod identities;
mod resources;

use notegate_core::Result;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct PurgeRepo {
    pool: PgPool,
}

impl PurgeRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Run each cleanup group in its own transaction, in order.
    ///
    /// A group error does not skip subsequent groups or undo committed work.
    /// Every failed group is logged; after all groups finish, return the first
    /// error in execution order. Counts are returned only when all groups commit.
    pub async fn run_once(&self) -> Result<PurgeRun> {
        // Await every group before propagating errors. Do not use `?` here:
        // independent cleanup must still run after an earlier group fails.
        let resources = resources::purge(&self.pool).await.inspect_err(|error| {
            tracing::warn!(event = "purge.group_failed", group = "resources", %error);
        });
        let identities = identities::purge(&self.pool).await.inspect_err(|error| {
            tracing::warn!(event = "purge.group_failed", group = "identities", %error);
        });
        let history = history::purge(&self.pool).await.inspect_err(|error| {
            tracing::warn!(event = "purge.group_failed", group = "history", %error);
        });

        let resources = resources?;
        let identities = identities?;
        let history = history?;

        Ok(PurgeRun {
            spaces_deleted: resources.spaces_deleted.max(0) as u64,
            nodes_deleted: resources.nodes_deleted.max(0) as u64,
            accounts_anonymized: identities.accounts_anonymized.max(0) as u64,
            api_keys_deleted: identities.api_keys_deleted.max(0) as u64,
            browser_sessions_deleted: identities.browser_sessions_deleted.max(0) as u64,
            object_storage_history_deleted: history.object_storage_history_deleted.max(0) as u64,
            audit_events_deleted: history.audit_events_deleted.max(0) as u64,
            file_change_events_deleted: history.file_change_events_deleted.max(0) as u64,
            command_invocations_deleted: history.command_invocations_deleted.max(0) as u64,
            link_graph_projections_deleted: resources.link_graph_projections_deleted.max(0) as u64,
            object_deletions_queued: resources.object_deletions_queued,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeRun {
    pub spaces_deleted: u64,
    pub nodes_deleted: u64,
    pub accounts_anonymized: u64,
    pub api_keys_deleted: u64,
    pub browser_sessions_deleted: u64,
    pub object_storage_history_deleted: u64,
    pub audit_events_deleted: u64,
    pub file_change_events_deleted: u64,
    pub command_invocations_deleted: u64,
    pub link_graph_projections_deleted: u64,
    pub object_deletions_queued: u64,
}
