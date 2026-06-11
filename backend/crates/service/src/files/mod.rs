//! File-tree feature: command inputs, output views, validation, the permission gate,
//! the patch engine, and the [`FilesService`].
//!
//! Command semantics follow the shared file-command spec
//! (`docs/spec/files-commands.md`) and are exposed through REST/MCP-specific
//! contracts. The service owns authorization, validation, and command
//! orchestration over the concrete database repository. Paths are derived from
//! parent links — never stored.

pub mod content;
pub mod patch;
pub mod policy;
pub mod target;
pub mod types;
pub mod validation;

mod mutate;
mod read;
mod view;

pub use patch::{PatchError, apply_edits};
pub use policy::FileCommand;
pub use target::{Target, parse_target};
pub use types::{
    ChildrenCursor, ChildrenPage, DeleteResult, NodeView, PatchResult, ReadContent, ReadResult,
    ReadTextBody, StoredContent, TextStats, TextView,
};
pub use types::{
    ChildrenRequest, CreateFolder, CreateText, DeleteNode, Edit, MoveNode, PatchText, ReadText,
    WriteTarget, WriteText, WriteTextBody,
};
pub use validation::FilesValidationError;

use notegate_core::limits::Limits;
use notegate_db::FilesRepo;
use notegate_model::{Node, NodeKind, Permission, TextObject};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};

/// File-tree command service: `ls` / `stat` / `mkdir` / `touch` / `read` /
/// `write` / `patch` / `mv` / `rm`.
///
/// Every command takes `(caller_account_id, space_id, ...)`. The service:
///
/// 1. Resolves the caller's live [`Permission`] through the repository permission lookup FIRST. No
///    live permission ⇒ not-found (`404`, hides the space); insufficient permission ⇒
///    forbidden (`403`, via [`policy::require`]).
/// 2. Validates input format (name, `.md`, depth, path length, text size)
///    with the pure [`validation`] functions.
/// 3. Pre-checks capacity limits (fanout, node/text counts, total bytes,
///    subtree-delete size) using counts read from the store, returning a typed
///    conflict. The DB layer re-enforces these in-transaction for race safety;
///    the service pre-check keeps the logic testable and the errors precise.
/// 4. Calls the store mutation, attributing it to the caller.
///
/// Paths are never stored on a node — the display path is derived from parents;
/// `move`/`rename` change only the moved node's `parent_id`/`name`.
#[derive(Debug, Clone)]
pub struct FilesService {
    store: FilesRepo,
    limits: Limits,
}

impl FilesService {
    pub fn new(store: FilesRepo) -> Self {
        Self::with_limits(store, Limits::default())
    }

    pub fn with_limits(store: FilesRepo, limits: Limits) -> Self {
        Self { store, limits }
    }
}

impl FilesService {
    // --- internal helpers ---

    /// Resolve the caller's permission (none ⇒ 404) and gate by command
    /// (insufficient permission ⇒ 403).
    pub(super) async fn authorize(
        &self,
        space_id: Uuid,
        account_id: Uuid,
        command: FileCommand,
    ) -> ServiceResult<Permission> {
        let permission = self
            .store
            .permission_for(space_id, account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        policy::require(permission, command)?;
        Ok(permission)
    }

    /// Load a live node or 404.
    pub(super) async fn load_node(&self, space_id: Uuid, node_id: Uuid) -> ServiceResult<Node> {
        self.store
            .find_node(space_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))
    }

    /// Load a live text, distinguishing a folder from a missing text.
    pub(super) async fn load_text(
        &self,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<(Node, TextObject)> {
        if let Some(text) = self.store.find_text(space_id, node_id).await? {
            return Ok(text);
        }

        if let Some(node) = self.store.find_node(space_id, node_id).await?
            && node.kind == NodeKind::Folder
        {
            return Err(ServiceError::InvalidInput(
                "target is a folder, not a text".to_owned(),
            ));
        }

        Err(ServiceError::NotFound("text not found".to_owned()))
    }

    /// The derived path of a node or 404.
    pub(super) async fn path_of(&self, space_id: Uuid, node_id: Uuid) -> ServiceResult<String> {
        self.store
            .node_path(space_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))
    }

    /// Shared create pre-checks for mkdir/touch/write-create: parent is a live
    /// folder, no sibling-name conflict, resulting depth + path length within
    /// limits, parent fanout and space node count within limits. Returns the
    /// parent's derived path.
    pub(super) async fn prepare_create(
        &self,
        space_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> ServiceResult<String> {
        let parent = self.load_node(space_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(ServiceError::InvalidInput(
                "parent must be a folder".to_owned(),
            ));
        }

        // Name conflict against live siblings.
        if self
            .store
            .find_live_child_by_name(space_id, parent_node_id, name)
            .await?
            .is_some()
        {
            return Err(ServiceError::Conflict(format!(
                "a node named '{name}' already exists in this folder"
            )));
        }

        let parent_path = self.path_of(space_id, parent_node_id).await?;
        let parent_depth = path_depth(&parent_path);
        let new_path = join_path(&parent_path, name);
        validation::validate_depth(parent_depth + 1)?;
        validation::validate_path_len(&new_path)?;

        let children = self
            .store
            .count_live_children(space_id, parent_node_id)
            .await?;
        validation::validate_fanout(children, self.limits)?;

        let nodes = self.store.count_live_nodes(space_id).await?;
        validation::validate_space_node_count(nodes, self.limits)?;

        Ok(parent_path)
    }
}

/// Join a parent path and a child name into a canonical path (root-aware).
pub(super) fn join_path(parent_path: &str, name: &str) -> String {
    if parent_path == "/" {
        format!("/{name}")
    } else {
        format!("{parent_path}/{name}")
    }
}

/// Depth (segment count below root) of a derived path. Root (`/`) is 0.
pub(super) fn path_depth(path: &str) -> usize {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .count()
}
