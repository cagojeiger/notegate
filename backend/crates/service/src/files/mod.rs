//! File-tree application service.
//!
//! This layer owns authorization, request validation, and orchestration. Database
//! transactions own structural invariants and quotas; paths are derived from parents.

pub mod content;
pub mod patch;
pub mod policy;
pub mod target;
pub mod validation;

mod events;
mod format;
mod mutate;
mod preview;
mod read;
mod tree;
mod view;

pub use format::validate_structured_text;
pub use notegate_model::files::{
    AppendText, BatchChildrenRequest, BatchChildrenResult, BeginObjectUpload,
    CanonicalChildrenPage, CanonicalNodeListPage, ChildrenCursor, ChildrenPage, ChildrenRequest,
    CopyCounts, CopyNode, CopyResult, CreateFolder, CreateText, DeleteNode, DeleteResult, Edit,
    EditText, FileStats, FileView, LineEdit, ListNodesRequest, MoveNode, NodeListCursor,
    NodeListPage, NodeListSort, NodeReveal, NodeSummaryView, NodeView, PatchMode, PatchResult,
    PatchText, PendingObjectUpload, ReadContent, ReadResult, ReadText, ReadTextBody, StoredContent,
    TextStats, TextView, UpdateNode, UpdateNodeSearchPolicy, UpdateNodeWriteLock,
    UpdateTextEncryption, WriteLockSource, WriteTarget, WriteText, WriteTextBody,
};
pub use notegate_model::search::{TreePage, TreeRequest};
pub use notegate_model::{
    AccountKind, FileChangeEvent, FileChangeEventCursor, FileChangeEventIdCursor,
    FileChangeEventPage, FileChangeSyncPage, ListFileChangeEvents, ListFileChangeEventsById,
    SyncFileChanges,
};
pub use patch::{PatchError, apply_edits};
pub use policy::FileCommand;
pub use preview::{BatchPreviewCandidate, MAX_BATCH_PREVIEW_PATH_BYTES, MAX_BATCH_PREVIEW_PATHS};
pub use read::MAX_BATCH_CHILDREN_PARENTS;
pub use target::{Target, parse_target};
pub use validation::FilesValidationError;
pub(crate) use view::hydrate_node_views;

use notegate_db::FilesRepo;
use notegate_model::{Caller, Channel, Node, NodeKind, Permission, TextObject};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};

/// File-tree service for Node, Text, metadata, and object-upload operations.
/// Authorization and request validation are service-owned; state-dependent invariants stay in DB transactions.
#[derive(Debug, Clone)]
pub struct FilesService {
    store: FilesRepo,
}

impl FilesService {
    pub fn new(store: FilesRepo) -> Self {
        Self { store }
    }
}

impl FilesService {
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

    pub(super) async fn authorize_space_owner_user(
        &self,
        caller_kind: AccountKind,
        account_id: Uuid,
        space_id: Uuid,
    ) -> ServiceResult<()> {
        if caller_kind != AccountKind::User {
            return Err(ServiceError::Forbidden(
                "only the space owner user can change node policy".to_owned(),
            ));
        }
        self.store
            .permission_for(space_id, account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        Ok(())
    }

    pub(super) async fn authorize_space_owner_dashboard_user(
        &self,
        caller: &Caller,
        space_id: Uuid,
    ) -> ServiceResult<()> {
        if caller.account.kind != AccountKind::User || caller.channel != Channel::Browser {
            return Err(ServiceError::Forbidden(
                "only the space owner in the dashboard can change node write locks".to_owned(),
            ));
        }
        if self
            .store
            .permission_for(space_id, caller.account_id())
            .await?
            .is_none()
        {
            return Err(ServiceError::Forbidden(
                "only the space owner in the dashboard can change node write locks".to_owned(),
            ));
        }
        Ok(())
    }

    pub(super) async fn load_node(&self, space_id: Uuid, node_id: Uuid) -> ServiceResult<Node> {
        self.store
            .find_node(space_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))
    }

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

    pub(super) async fn path_of(&self, space_id: Uuid, node_id: Uuid) -> ServiceResult<String> {
        self.store
            .node_path(space_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))
    }
}

pub(super) fn join_path(parent_path: &str, name: &str) -> String {
    if parent_path == "/" {
        format!("/{name}")
    } else {
        format!("{parent_path}/{name}")
    }
}
