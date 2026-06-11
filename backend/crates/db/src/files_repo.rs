//! Nodes + text-object persistence (the file tree) and search.
//!
//! Exposes concrete persistence methods consumed by `notegate-service`.
//! All queries use runtime-checked `query_as::<_, Row>()` / `query()` — never the
//! `query!` macro — so a schema reset never breaks compilation. Every mutation
//! runs in one transaction and sets attribution (created_by/updated_by/
//! deleted_by) from the caller.
//!
//! Nodes have NO stored path column. Display paths and scoped search paths are
//! derived by space-bounded recursive CTEs (see `files::queries`);
//! move/rename updates only the moved node's row (O(1), no descendant rewrite).

use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_core::limits::Limits;
use notegate_model::{Node, NodeKind, Permission, TextObject};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use notegate_model::files::{ChildrenCursor, CreateFolder, MoveNode, StoredContent, TextStats};
use notegate_model::search::{FindCursor, GrepCandidate, GrepCursor};

use crate::files::{commands, queries};

#[derive(Debug, Clone)]
pub struct FilesRepo {
    pool: PgPool,
    limits: Limits,
}

impl FilesRepo {
    pub fn new(pool: PgPool) -> Self {
        Self::with_limits(pool, Limits::default())
    }

    pub fn with_limits(pool: PgPool, limits: Limits) -> Self {
        Self { pool, limits }
    }
}

impl FilesRepo {
    pub async fn find_node(&self, space_id: Uuid, node_id: Uuid) -> Result<Option<Node>> {
        queries::node::find_node(&self.pool, space_id, node_id).await
    }

    pub async fn node_path(&self, space_id: Uuid, node_id: Uuid) -> Result<Option<String>> {
        queries::node::node_path(&self.pool, space_id, node_id).await
    }

    pub async fn resolve_path(&self, space_id: Uuid, path: &str) -> Result<Option<Uuid>> {
        queries::search::resolve_scope_node(&self.pool, space_id, path).await
    }

    pub async fn has_children(&self, space_id: Uuid, node_id: Uuid) -> Result<bool> {
        queries::node::has_children(&self.pool, space_id, node_id).await
    }

    pub async fn count_live_children(&self, space_id: Uuid, parent_node_id: Uuid) -> Result<usize> {
        queries::node::count_live_children(&self.pool, space_id, parent_node_id).await
    }

    pub async fn find_live_child_by_name(
        &self,
        space_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> Result<Option<Node>> {
        queries::node::find_live_child_by_name(&self.pool, space_id, parent_node_id, name).await
    }

    pub async fn count_live_nodes(&self, space_id: Uuid) -> Result<usize> {
        queries::node::count_live_nodes(&self.pool, space_id).await
    }

    pub async fn count_live_texts(&self, space_id: Uuid) -> Result<usize> {
        queries::text::count_live_texts(&self.pool, space_id).await
    }

    pub async fn sum_live_text_bytes(&self, space_id: Uuid) -> Result<usize> {
        queries::text::sum_live_text_bytes(&self.pool, space_id).await
    }

    pub async fn text_stats(&self, space_id: Uuid, node_id: Uuid) -> Result<Option<TextStats>> {
        queries::text::text_stats(&self.pool, space_id, node_id).await
    }

    pub async fn find_text(
        &self,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<(Node, TextObject)>> {
        queries::text::find_text(&self.pool, space_id, node_id).await
    }

    pub async fn paged_children(
        &self,
        space_id: Uuid,
        parent_node_id: Uuid,
        limit: i64,
        cursor: Option<&ChildrenCursor>,
    ) -> Result<(Vec<Node>, bool)> {
        let cursor = cursor.map(|c| (c.sort_order, c.name.as_str(), c.id));
        queries::node::paged_children(&self.pool, space_id, parent_node_id, limit, cursor).await
    }

    pub async fn subtree_relative_depth(&self, space_id: Uuid, node_id: Uuid) -> Result<usize> {
        queries::node::subtree_relative_depth(&self.pool, space_id, node_id).await
    }

    pub async fn subtree_live_count(&self, space_id: Uuid, node_id: Uuid) -> Result<usize> {
        queries::node::subtree_live_count(&self.pool, space_id, node_id).await
    }

    pub async fn is_self_or_descendant(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        candidate_id: Uuid,
    ) -> Result<bool> {
        queries::node::is_self_or_descendant(&self.pool, space_id, node_id, candidate_id).await
    }

    pub async fn insert_folder(
        &self,
        space_id: Uuid,
        command: &CreateFolder,
        created_by: Uuid,
    ) -> Result<Node> {
        commands::create::insert_folder(
            &self.pool,
            space_id,
            command.parent_node_id,
            &command.name,
            created_by,
            self.limits,
        )
        .await
    }

    pub async fn insert_text(
        &self,
        space_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
        content: &StoredContent,
        created_by: Uuid,
    ) -> Result<(Node, TextObject)> {
        commands::create::insert_text(
            &self.pool,
            space_id,
            parent_node_id,
            name,
            content,
            created_by,
            self.limits,
        )
        .await
    }

    pub async fn save_text_content(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        content: &StoredContent,
        expected_sha256: Option<&str>,
        updated_by: Uuid,
    ) -> Result<(Node, TextObject)> {
        commands::save::save_text_content(
            &self.pool,
            space_id,
            node_id,
            content,
            expected_sha256,
            updated_by,
            self.limits,
        )
        .await
    }

    pub async fn move_node(
        &self,
        space_id: Uuid,
        command: &MoveNode,
        updated_by: Uuid,
    ) -> Result<Node> {
        commands::move_node::move_node(commands::move_node::MoveNodeArgs {
            pool: &self.pool,
            space_id,
            node_id: command.node_id,
            new_parent_id: command.new_parent_node_id,
            new_name: command.new_name.as_deref(),
            expected_parent_id: command.expected_parent_id,
            updated_by,
            caps: self.limits,
        })
        .await
    }

    pub async fn update_node_metadata(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        new_name: Option<&str>,
        new_sort_order: Option<i32>,
        updated_by: Uuid,
    ) -> Result<Node> {
        commands::update::update_node_metadata(
            &self.pool,
            space_id,
            node_id,
            new_name,
            new_sort_order,
            updated_by,
        )
        .await
    }

    pub async fn replace_node_metadata(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        metadata: &Value,
        updated_by: Uuid,
    ) -> Result<Node> {
        commands::update::replace_node_metadata(&self.pool, space_id, node_id, metadata, updated_by)
            .await
    }

    pub async fn soft_delete_node(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        deleted_by: Uuid,
    ) -> Result<DateTime<Utc>> {
        commands::delete::soft_delete_node(&self.pool, space_id, node_id, deleted_by).await
    }
}

impl FilesRepo {
    pub async fn permission_for(
        &self,
        space_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<Permission>> {
        queries::node::permission_for(&self.pool, space_id, account_id).await
    }

    pub async fn find_nodes(
        &self,
        space_id: Uuid,
        q: &str,
        scope: Option<&str>,
        kind: Option<NodeKind>,
        limit: i64,
        cursor: Option<&FindCursor>,
    ) -> Result<Vec<(Node, String, bool)>> {
        let scope_node = self.resolve_scope(space_id, scope).await?;
        if scope.is_some() && scope_node.is_none() {
            return Err(notegate_core::Error::not_found("scope path not found"));
        }
        queries::search::find_nodes(&self.pool, space_id, q, scope_node, kind, limit, cursor).await
    }

    pub async fn grep_candidates(
        &self,
        space_id: Uuid,
        q: &str,
        scope: Option<&str>,
        limit: i64,
        cursor: Option<&GrepCursor>,
    ) -> Result<Vec<GrepCandidate>> {
        let scope_node = self.resolve_scope(space_id, scope).await?;
        if scope.is_some() && scope_node.is_none() {
            return Err(notegate_core::Error::not_found("scope path not found"));
        }
        queries::search::grep_candidates(&self.pool, space_id, q, scope_node, limit, cursor).await
    }
}

impl FilesRepo {
    /// Resolve an optional scope path to a live node id within the space.
    /// `None` scope means "whole space" (no subtree restriction).
    pub async fn resolve_scope(&self, space_id: Uuid, scope: Option<&str>) -> Result<Option<Uuid>> {
        match scope {
            None => Ok(None),
            Some(path) => queries::search::resolve_scope_node(&self.pool, space_id, path).await,
        }
    }
}
