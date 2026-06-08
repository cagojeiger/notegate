//! File-tree feature: command inputs, output views, validation, the role gate,
//! the patch engine, and the [`FilesService`] over a [`FilesStore`].
//!
//! Command semantics follow the shared file-command spec
//! (`docs/spec/files-commands.md`) and are exposed through REST/MCP-specific
//! contracts. The service is pure logic plus the store/authorization trait; the
//! `db` crate implements [`FilesStore`]. Paths are derived from parent links —
//! never stored.

pub mod content;
pub mod patch;
pub mod policy;
pub mod store;
pub mod target;
pub mod types;
pub mod validation;

mod mutate;
mod read;
mod view;

pub use content::{Metrics, compute as content_metrics};
pub use patch::{PatchError, apply_edits};
pub use policy::{FileCommand, require as require_role};
pub use store::FilesStore;
pub use target::{Target, parse_target};
pub use types::{
    ChildrenCursor, ChildrenPage, DeleteResult, DocumentStats, DocumentView, NodeView, PatchResult,
    ReadContent, ReadResult, StoredContent,
};
pub use types::{
    ChildrenRequest, CreateDocument, CreateFolder, DeleteNode, Edit, MoveNode, PatchDocument,
    ReadDocument, WriteDocument, WriteTarget,
};
pub use validation::FilesValidationError;

#[cfg(test)]
use notegate_core::limits;
use notegate_core::limits::Limits;
use notegate_model::{Document, Node, NodeKind, Role};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};

/// File-tree command service: `ls` / `stat` / `mkdir` / `touch` / `read` /
/// `write` / `patch` / `mv` / `rm`.
///
/// Every command takes `(caller_account_id, workspace_id, ...)`. The service:
///
/// 1. Resolves the caller's live [`Role`] via [`FilesStore::role_for`] FIRST. No
///    live role ⇒ not-found (`404`, hides the workspace); an insufficient role ⇒
///    forbidden (`403`, via [`policy::require`]).
/// 2. Validates input format (name, `.md`, depth, path length, document size)
///    with the pure [`validation`] functions.
/// 3. Pre-checks capacity limits (fanout, node/document counts, total bytes,
///    subtree-delete size) using counts read from the store, returning a typed
///    conflict. The DB layer re-enforces these in-transaction for race safety;
///    the service pre-check keeps the logic testable and the errors precise.
/// 4. Calls the store mutation, attributing it to the caller.
///
/// Paths are never stored on a node — the display path is derived from parents;
/// `move`/`rename` change only the moved node's `parent_id`/`name`.
#[derive(Debug, Clone)]
pub struct FilesService<S> {
    store: S,
    limits: Limits,
}

impl<S> FilesService<S>
where
    S: FilesStore,
{
    pub fn new(store: S) -> Self {
        Self::with_limits(store, Limits::default())
    }

    pub fn with_limits(store: S, limits: Limits) -> Self {
        Self { store, limits }
    }
}

impl<S> FilesService<S>
where
    S: FilesStore,
{
    // --- internal helpers ---

    /// Resolve the caller's role (no role ⇒ 404) and gate by command (lesser
    /// role ⇒ 403).
    pub(super) async fn authorize(
        &self,
        workspace_id: Uuid,
        account_id: Uuid,
        command: FileCommand,
    ) -> ServiceResult<Role> {
        let role = self
            .store
            .role_for(workspace_id, account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("workspace not found".to_owned()))?;
        policy::require(role, command)?;
        Ok(role)
    }

    /// Load a live node or 404.
    pub(super) async fn load_node(&self, workspace_id: Uuid, node_id: Uuid) -> ServiceResult<Node> {
        self.store
            .find_node(workspace_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))
    }

    /// Load a live document, distinguishing a folder from a missing document.
    pub(super) async fn load_document(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<(Node, Document)> {
        if let Some(document) = self.store.find_document(workspace_id, node_id).await? {
            return Ok(document);
        }

        if let Some(node) = self.store.find_node(workspace_id, node_id).await?
            && node.kind == NodeKind::Folder
        {
            return Err(ServiceError::InvalidInput(
                "target is a folder, not a document".to_owned(),
            ));
        }

        Err(ServiceError::NotFound("document not found".to_owned()))
    }

    /// The derived path of a node or 404.
    pub(super) async fn path_of(&self, workspace_id: Uuid, node_id: Uuid) -> ServiceResult<String> {
        self.store
            .node_path(workspace_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))
    }

    /// Shared create pre-checks for mkdir/touch/write-create: parent is a live
    /// folder, no sibling-name conflict, resulting depth + path length within
    /// limits, parent fanout and workspace node count within limits. Returns the
    /// parent's derived path.
    pub(super) async fn prepare_create(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> ServiceResult<String> {
        let parent = self.load_node(workspace_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(ServiceError::InvalidInput(
                "parent must be a folder".to_owned(),
            ));
        }

        // Name conflict against live siblings.
        if self
            .store
            .find_live_child_by_name(workspace_id, parent_node_id, name)
            .await?
            .is_some()
        {
            return Err(ServiceError::Conflict(format!(
                "a node named '{name}' already exists in this folder"
            )));
        }

        let parent_path = self.path_of(workspace_id, parent_node_id).await?;
        let parent_depth = path_depth(&parent_path);
        let new_path = join_path(&parent_path, name);
        validation::validate_depth(parent_depth + 1)?;
        validation::validate_path_len(&new_path)?;

        let children = self
            .store
            .count_live_children(workspace_id, parent_node_id)
            .await?;
        validation::validate_fanout(children, self.limits)?;

        let nodes = self.store.count_live_nodes(workspace_id).await?;
        validation::validate_workspace_node_count(nodes, self.limits)?;

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

#[cfg(test)]
mod tests;
