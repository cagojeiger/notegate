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
use notegate_core::tier::effective_file_tree_limits;
use notegate_model::search::{SearchNodeCandidate, SearchTextCandidate};
use notegate_model::{FileObject, Node, NodeKind, Permission, TextObject};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

use crate::file_change_event_repo;
use crate::files::{commands, queries};
use crate::tier_lookup;
use notegate_model::files::{
    BeginObjectUpload, ChildrenCursor, CopyCounts, CopyNode, CreateFolder, FileStats, MoveNode,
    NodeListCursor, NodeListSort, ObjectUploadMode, ObjectUploadRegistration, PendingObjectUpload,
    StoredContent, TextStats,
};

#[derive(Debug, Clone)]
pub struct FilesRepo {
    pool: PgPool,
    limits: Limits,
}

#[derive(Debug, Clone, Copy)]
pub enum TextMutationKind {
    Write,
    Append,
    Patch,
    Edit,
}

impl TextMutationKind {
    pub(crate) fn op_type(self) -> &'static str {
        match self {
            TextMutationKind::Write => "text.write",
            TextMutationKind::Append => "text.append",
            TextMutationKind::Patch => "text.patch",
            TextMutationKind::Edit => "text.edit",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MetadataMutationKind {
    Replace,
    Patch,
}

impl MetadataMutationKind {
    pub(crate) fn op_type(self) -> &'static str {
        match self {
            MetadataMutationKind::Replace => "metadata.replace",
            MetadataMutationKind::Patch => "metadata.patch",
        }
    }
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
    pub async fn effective_limits_for_space(&self, space_id: Uuid) -> Result<Limits> {
        let tier = tier_lookup::active_space_owner_tier(&self.pool, space_id).await?;
        Ok(effective_file_tree_limits(tier, self.limits))
    }

    pub async fn find_node(&self, space_id: Uuid, node_id: Uuid) -> Result<Option<Node>> {
        queries::node::find_node(&self.pool, space_id, node_id).await
    }

    pub async fn node_path(&self, space_id: Uuid, node_id: Uuid) -> Result<Option<String>> {
        queries::node::node_path(&self.pool, space_id, node_id).await
    }

    pub async fn node_paths_many(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, String>> {
        queries::node::node_paths_many(&self.pool, space_id, node_ids).await
    }

    pub async fn ancestor_chain(&self, space_id: Uuid, node_id: Uuid) -> Result<Vec<Node>> {
        queries::node::ancestor_chain(&self.pool, space_id, node_id).await
    }

    pub async fn resolve_path(&self, space_id: Uuid, path: &str) -> Result<Option<Uuid>> {
        queries::search::resolve_scope_node(&self.pool, space_id, path).await
    }

    pub async fn has_children(&self, space_id: Uuid, node_id: Uuid) -> Result<bool> {
        queries::node::has_children(&self.pool, space_id, node_id).await
    }

    pub async fn has_children_many(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, bool>> {
        queries::node::has_children_many(&self.pool, space_id, node_ids).await
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

    pub async fn text_stats(&self, space_id: Uuid, node_id: Uuid) -> Result<Option<TextStats>> {
        queries::text::text_stats(&self.pool, space_id, node_id).await
    }

    pub async fn text_stats_many(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, TextStats>> {
        queries::text::text_stats_many(&self.pool, space_id, node_ids).await
    }

    pub async fn find_text(
        &self,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<(Node, TextObject)>> {
        queries::text::find_text(&self.pool, space_id, node_id).await
    }

    pub async fn find_texts(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, TextObject>> {
        queries::text::find_texts(&self.pool, space_id, node_ids).await
    }

    pub async fn file_stats(&self, space_id: Uuid, node_id: Uuid) -> Result<Option<FileStats>> {
        queries::file::file_stats(&self.pool, space_id, node_id).await
    }

    pub async fn file_stats_many(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, FileStats>> {
        queries::file::file_stats_many(&self.pool, space_id, node_ids).await
    }

    pub async fn find_file(
        &self,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<(Node, FileObject)>> {
        queries::file::find_file(&self.pool, space_id, node_id).await
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

    pub async fn paged_nodes(
        &self,
        space_id: Uuid,
        kind: Option<NodeKind>,
        sort: NodeListSort,
        limit: i64,
        cursor: Option<&NodeListCursor>,
    ) -> Result<(Vec<Node>, bool)> {
        let cursor = match cursor {
            Some(NodeListCursor::UpdatedAtDesc { updated_at, id, .. }) => {
                Some(queries::node::NodeListDbCursor::UpdatedAtDesc {
                    updated_at: *updated_at,
                    id: *id,
                })
            }
            Some(NodeListCursor::NameAsc { name, id, .. }) => {
                Some(queries::node::NodeListDbCursor::NameAsc {
                    name: name.as_str(),
                    id: *id,
                })
            }
            None => None,
        };
        queries::node::paged_nodes(&self.pool, space_id, kind, sort, limit, cursor).await
    }

    pub async fn list_file_change_events(
        &self,
        space_id: Uuid,
        node_id: Option<Uuid>,
        limit: i64,
        cursor: Option<&notegate_model::FileChangeEventCursor>,
    ) -> Result<Vec<notegate_model::FileChangeEvent>> {
        file_change_event_repo::list_file_change_events(
            &self.pool, space_id, node_id, limit, cursor,
        )
        .await
    }

    pub async fn search_node_candidates(
        &self,
        space_id: Uuid,
        scope_node_id: Uuid,
        scope_path: &str,
        after_sort_path: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchNodeCandidate>> {
        queries::search::node_candidates(
            &self.pool,
            space_id,
            scope_node_id,
            scope_path,
            after_sort_path,
            limit,
        )
        .await
    }

    pub async fn search_text_candidates(
        &self,
        space_id: Uuid,
        scope_node_id: Uuid,
        scope_path: &str,
        after_sort_path: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchTextCandidate>> {
        queries::search::text_candidates(
            &self.pool,
            space_id,
            scope_node_id,
            scope_path,
            after_sort_path,
            limit,
        )
        .await
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

    pub async fn insert_object_upload(
        &self,
        id: Uuid,
        object_key: &str,
        space_id: Uuid,
        requested_by: Uuid,
        input: &BeginObjectUpload,
    ) -> Result<PendingObjectUpload> {
        let registration = ObjectUploadRegistration {
            id,
            object_key: object_key.to_owned(),
            upload_mode: ObjectUploadMode::Single,
            multipart_upload_id: None,
            multipart_part_size: None,
        };
        self.insert_registered_object_upload(&registration, space_id, requested_by, input)
            .await
    }

    pub async fn insert_registered_object_upload(
        &self,
        registration: &ObjectUploadRegistration,
        space_id: Uuid,
        requested_by: Uuid,
        input: &BeginObjectUpload,
    ) -> Result<PendingObjectUpload> {
        crate::files::object_uploads::insert(
            &self.pool,
            registration,
            space_id,
            requested_by,
            input,
            self.limits,
        )
        .await
    }

    pub async fn object_upload(
        &self,
        id: Uuid,
        space_id: Uuid,
        requested_by: Uuid,
    ) -> Result<Option<PendingObjectUpload>> {
        crate::files::object_uploads::find(&self.pool, id, space_id, requested_by).await
    }

    pub async fn object_upload_for_caller(
        &self,
        id: Uuid,
        requested_by: Uuid,
    ) -> Result<Option<PendingObjectUpload>> {
        crate::files::object_uploads::find_for_caller(&self.pool, id, requested_by).await
    }

    pub async fn touch_object_upload(
        &self,
        id: Uuid,
        space_id: Uuid,
        requested_by: Uuid,
    ) -> Result<bool> {
        crate::files::object_uploads::touch(&self.pool, id, space_id, requested_by).await
    }

    pub async fn request_object_upload_expiry(
        &self,
        id: Uuid,
        space_id: Uuid,
        requested_by: Uuid,
    ) -> Result<bool> {
        crate::files::object_uploads::request_expiry(&self.pool, id, space_id, requested_by).await
    }

    pub async fn attach_object_upload(
        &self,
        id: Uuid,
        space_id: Uuid,
        requested_by: Uuid,
    ) -> Result<(Node, FileObject)> {
        crate::files::object_uploads::attach(&self.pool, id, space_id, requested_by, self.limits)
            .await
    }

    pub async fn save_text_content(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        content: &StoredContent,
        expected_sha256: Option<&str>,
        updated_by: Uuid,
        mutation_kind: TextMutationKind,
    ) -> Result<(Node, TextObject)> {
        commands::save::save_text_content(commands::save::SaveTextContentArgs {
            pool: &self.pool,
            space_id,
            node_id,
            content,
            expected_sha256,
            updated_by,
            mutation_kind,
            caps: self.limits,
        })
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

    pub async fn copy_node(
        &self,
        space_id: Uuid,
        command: &CopyNode,
        created_by: Uuid,
    ) -> Result<(Node, CopyCounts)> {
        commands::copy_node::copy_node(commands::copy_node::CopyNodeArgs {
            pool: &self.pool,
            space_id,
            source_node_id: command.node_id,
            new_parent_id: command.new_parent_node_id,
            new_name: &command.new_name,
            recursive: command.recursive,
            created_by,
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
        mutation_kind: MetadataMutationKind,
    ) -> Result<Node> {
        commands::update::replace_node_metadata(
            &self.pool,
            space_id,
            node_id,
            metadata,
            updated_by,
            mutation_kind,
        )
        .await
    }

    pub async fn soft_delete_node(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        deleted_by: Uuid,
        recursive: bool,
    ) -> Result<DateTime<Utc>> {
        commands::delete::soft_delete_node(&self.pool, space_id, node_id, deleted_by, recursive)
            .await
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
