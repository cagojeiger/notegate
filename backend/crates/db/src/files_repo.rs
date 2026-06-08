//! Nodes + documents persistence (the file tree) and search.
//!
//! Implements the `notegate-service` [`FilesStore`] and [`SearchStore`] traits.
//! All queries use runtime-checked `query_as::<_, Row>()` / `query()` — never the
//! `query!` macro — so a schema reset never breaks compilation. Every mutation
//! runs in one transaction and sets attribution (created_by/updated_by/
//! deleted_by) from the caller.
//!
//! Nodes have NO stored path column. Display paths and scoped search paths are
//! derived by workspace-bounded recursive CTEs (see `files::queries`);
//! move/rename updates only the moved node's row (O(1), no descendant rewrite).

use notegate_core::Result;
use notegate_model::{Document, Node, NodeKind, Role};
use sqlx::PgPool;
use uuid::Uuid;

use notegate_service::files::{ChildrenCursor, CreateFolder, FilesStore, MoveNode, StoredContent};
use notegate_service::search::{FindCursor, GrepCandidate, GrepCursor, SearchStore};

use crate::files::{commands, queries};

#[derive(Debug, Clone)]
pub struct FilesRepo {
    pool: PgPool,
}

impl FilesRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl FilesStore for FilesRepo {
    async fn role_for(&self, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
        queries::node::role_for(&self.pool, workspace_id, account_id).await
    }

    async fn root_node(&self, workspace_id: Uuid) -> Result<Node> {
        queries::node::root_node(&self.pool, workspace_id).await
    }

    async fn find_node(&self, workspace_id: Uuid, node_id: Uuid) -> Result<Option<Node>> {
        queries::node::find_node(&self.pool, workspace_id, node_id).await
    }

    async fn find_deleted_node(&self, workspace_id: Uuid, node_id: Uuid) -> Result<Option<Node>> {
        queries::node::find_deleted_node(&self.pool, workspace_id, node_id).await
    }

    async fn node_path(&self, workspace_id: Uuid, node_id: Uuid) -> Result<Option<String>> {
        queries::node::node_path(&self.pool, workspace_id, node_id).await
    }

    async fn resolve_path(&self, workspace_id: Uuid, path: &str) -> Result<Option<Uuid>> {
        queries::search::resolve_scope_node(&self.pool, workspace_id, path).await
    }

    async fn has_children(&self, workspace_id: Uuid, node_id: Uuid) -> Result<bool> {
        queries::node::has_children(&self.pool, workspace_id, node_id).await
    }

    async fn count_live_children(&self, workspace_id: Uuid, parent_node_id: Uuid) -> Result<usize> {
        queries::node::count_live_children(&self.pool, workspace_id, parent_node_id).await
    }

    async fn find_live_child_by_name(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> Result<Option<Node>> {
        queries::node::find_live_child_by_name(&self.pool, workspace_id, parent_node_id, name).await
    }

    async fn count_live_nodes(&self, workspace_id: Uuid) -> Result<usize> {
        queries::node::count_live_nodes(&self.pool, workspace_id).await
    }

    async fn count_live_documents(&self, workspace_id: Uuid) -> Result<usize> {
        queries::document::count_live_documents(&self.pool, workspace_id).await
    }

    async fn sum_live_document_bytes(&self, workspace_id: Uuid) -> Result<usize> {
        queries::document::sum_live_document_bytes(&self.pool, workspace_id).await
    }

    async fn find_document(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<(Node, Document)>> {
        queries::document::find_document(&self.pool, workspace_id, node_id).await
    }

    async fn paged_children(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        limit: i64,
        cursor: Option<&ChildrenCursor>,
    ) -> Result<(Vec<Node>, bool)> {
        let cursor = cursor.map(|c| (c.sort_order, c.name.as_str(), c.id));
        queries::node::paged_children(&self.pool, workspace_id, parent_node_id, limit, cursor).await
    }

    async fn subtree_relative_depth(&self, workspace_id: Uuid, node_id: Uuid) -> Result<usize> {
        queries::node::subtree_relative_depth(&self.pool, workspace_id, node_id).await
    }

    async fn subtree_live_count(&self, workspace_id: Uuid, node_id: Uuid) -> Result<usize> {
        queries::node::subtree_live_count(&self.pool, workspace_id, node_id).await
    }

    async fn is_self_or_descendant(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        candidate_id: Uuid,
    ) -> Result<bool> {
        queries::node::is_self_or_descendant(&self.pool, workspace_id, node_id, candidate_id).await
    }

    async fn has_deleted_ancestor(&self, workspace_id: Uuid, node_id: Uuid) -> Result<bool> {
        queries::node::has_deleted_ancestor(&self.pool, workspace_id, node_id).await
    }

    async fn insert_folder(
        &self,
        workspace_id: Uuid,
        command: &CreateFolder,
        created_by: Uuid,
    ) -> Result<Node> {
        commands::create::insert_folder(
            &self.pool,
            workspace_id,
            command.parent_node_id,
            &command.name,
            created_by,
        )
        .await
    }

    async fn insert_document(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
        content: &StoredContent,
        created_by: Uuid,
    ) -> Result<(Node, Document)> {
        commands::create::insert_document(
            &self.pool,
            workspace_id,
            parent_node_id,
            name,
            content,
            created_by,
        )
        .await
    }

    async fn save_document_content(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        content: &StoredContent,
        updated_by: Uuid,
    ) -> Result<(Node, Document)> {
        commands::save::save_document_content(
            &self.pool,
            workspace_id,
            node_id,
            content,
            updated_by,
        )
        .await
    }

    async fn move_node(
        &self,
        workspace_id: Uuid,
        command: &MoveNode,
        updated_by: Uuid,
    ) -> Result<Node> {
        commands::move_node::move_node(
            &self.pool,
            workspace_id,
            command.node_id,
            command.new_parent_node_id,
            command.new_name.as_deref(),
            updated_by,
        )
        .await
    }

    async fn update_node_metadata(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        new_name: Option<&str>,
        new_sort_order: Option<i32>,
        updated_by: Uuid,
    ) -> Result<Node> {
        commands::update::update_node_metadata(
            &self.pool,
            workspace_id,
            node_id,
            new_name,
            new_sort_order,
            updated_by,
        )
        .await
    }

    async fn soft_delete_node(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        deleted_by: Uuid,
    ) -> Result<()> {
        commands::delete::soft_delete_node(&self.pool, workspace_id, node_id, deleted_by).await
    }

    async fn restore_node(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        restored_by: Uuid,
    ) -> Result<Node> {
        commands::restore::restore_node(&self.pool, workspace_id, node_id, restored_by).await
    }
}

impl SearchStore for FilesRepo {
    async fn role_for(&self, workspace_id: Uuid, account_id: Uuid) -> Result<Option<Role>> {
        queries::node::role_for(&self.pool, workspace_id, account_id).await
    }

    async fn find_nodes(
        &self,
        workspace_id: Uuid,
        q: &str,
        scope: Option<&str>,
        kind: Option<NodeKind>,
        limit: i64,
        cursor: Option<&FindCursor>,
    ) -> Result<Vec<(Node, String, bool)>> {
        let scope_node = self.resolve_scope(workspace_id, scope).await?;
        // A scope path that does not resolve yields no results (empty subtree).
        if scope.is_some() && scope_node.is_none() {
            return Ok(Vec::new());
        }
        queries::search::find_nodes(&self.pool, workspace_id, q, scope_node, kind, limit, cursor)
            .await
    }

    async fn grep_candidates(
        &self,
        workspace_id: Uuid,
        q: &str,
        scope: Option<&str>,
        limit: i64,
        cursor: Option<&GrepCursor>,
    ) -> Result<Vec<GrepCandidate>> {
        let scope_node = self.resolve_scope(workspace_id, scope).await?;
        if scope.is_some() && scope_node.is_none() {
            return Ok(Vec::new());
        }
        queries::search::grep_candidates(&self.pool, workspace_id, q, scope_node, limit, cursor)
            .await
    }
}

impl FilesRepo {
    /// Resolve an optional scope path to a live node id within the workspace.
    /// `None` scope means "whole workspace" (no subtree restriction).
    async fn resolve_scope(&self, workspace_id: Uuid, scope: Option<&str>) -> Result<Option<Uuid>> {
        match scope {
            None => Ok(None),
            Some(path) => queries::search::resolve_scope_node(&self.pool, workspace_id, path).await,
        }
    }
}
