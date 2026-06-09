//! Workspace lifecycle: create / get / rename / delete.
//!
//! POLICY: `create` is user-only and relies on the DB trigger to materialize
//! the canonical root node. The creator user is the implicit lifecycle owner;
//! `workspace_access` stores only viewer/editor grants. `get` is visible to any
//! effective role; `rename`/`delete` require owner. A workspace the caller cannot
//! see is reported as not-found so the api returns `404`.

use notegate_core::limits;
use notegate_core::validation::validate_workspace_name;
use notegate_db::WorkspaceRepo;
use notegate_model::{AccountKind, Role};
pub use notegate_model::{
    CreateWorkspace, ListWorkspaces, RenameWorkspace, WorkspaceCursor, WorkspacePage, WorkspaceView,
};
use uuid::Uuid;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::pagination::clamp_limit;

/// Workspace lifecycle service.
#[derive(Debug, Clone)]
pub struct WorkspaceService {
    store: WorkspaceRepo,
}

impl WorkspaceService {
    pub fn new(store: WorkspaceRepo) -> Self {
        Self { store }
    }

    /// Create a workspace owned by the authenticated user caller. Enforces the
    /// creator quota and a clean name conflict.
    pub async fn create(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        command: CreateWorkspace,
    ) -> ServiceResult<WorkspaceView> {
        require_user_caller(caller_kind)?;
        validate_workspace_name(&command.name)?;

        let workspace = self
            .store
            .create_workspace(caller_account_id, &command)
            .await?;

        let root_node_id = self
            .store
            .root_node_id(workspace.id)
            .await?
            .ok_or_else(|| {
                ServiceError::Internal("workspace root node was not created".to_owned())
            })?;

        Ok(WorkspaceView {
            workspace,
            role: Role::Owner,
            root_node_id,
        })
    }

    /// Get a workspace visible to the caller (any non-revoked role). Hidden as
    /// not-found when the caller has no live role.
    pub async fn get(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
    ) -> ServiceResult<WorkspaceView> {
        let role = self.require_role(workspace_id, caller_account_id).await?;

        let workspace = self
            .store
            .find_workspace(workspace_id)
            .await?
            .ok_or_else(not_found)?;
        let root_node_id = self
            .store
            .root_node_id(workspace_id)
            .await?
            .ok_or_else(not_found)?;

        Ok(WorkspaceView {
            workspace,
            role,
            root_node_id,
        })
    }

    /// List the workspaces the caller can see, with role and root node id.
    pub async fn list(
        &self,
        caller_account_id: Uuid,
        request: ListWorkspaces,
    ) -> ServiceResult<WorkspacePage> {
        let limit = clamp_limit(
            request.limit,
            limits::WORKSPACES_DEFAULT_LIMIT,
            limits::WORKSPACES_MAX_LIMIT,
        );
        let cursor = match request.cursor.as_deref() {
            None => None,
            Some(raw) => Some(
                cursor::decode::<WorkspaceCursor>(raw)
                    .map_err(|_error| ServiceError::InvalidInput("invalid cursor".to_owned()))?,
            ),
        };

        let mut items = self
            .store
            .list_workspace_views_for(caller_account_id, limit + 1, cursor.as_ref())
            .await?;
        let has_more = items.len() as i64 > limit;
        items.truncate(limit as usize);
        let next_cursor = if has_more {
            items
                .last()
                .map(|view| WorkspaceCursor {
                    created_at: view.workspace.created_at,
                    id: view.workspace.id,
                })
                .map(|cursor| cursor::encode(&cursor))
                .transpose()
                .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
        } else {
            None
        };

        Ok(WorkspacePage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    /// Load one workspace view by id, if visible to the caller.
    pub async fn find_visible_by_id(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
    ) -> ServiceResult<Option<WorkspaceView>> {
        Ok(self
            .store
            .find_workspace_view_for(caller_account_id, workspace_id)
            .await?)
    }

    /// Load exact-name workspace matches visible to the caller.
    pub async fn find_visible_by_name(
        &self,
        caller_account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> ServiceResult<Vec<WorkspaceView>> {
        validate_workspace_name(name)?;
        let limit = clamp_limit(Some(limit), 1, limits::WORKSPACES_MAX_LIMIT);
        Ok(self
            .store
            .list_workspace_views_by_name_for(caller_account_id, name, limit)
            .await?)
    }

    /// Rename a workspace. Requires `owner`.
    pub async fn rename(
        &self,
        caller_account_id: Uuid,
        command: RenameWorkspace,
    ) -> ServiceResult<WorkspaceView> {
        validate_workspace_name(&command.new_name)?;
        self.require_owner(command.workspace_id, caller_account_id)
            .await?;

        let workspace = self
            .store
            .rename_workspace(command.workspace_id, &command.new_name)
            .await?;
        let root_node_id = self
            .store
            .root_node_id(command.workspace_id)
            .await?
            .ok_or_else(not_found)?;

        Ok(WorkspaceView {
            workspace,
            role: Role::Owner,
            root_node_id,
        })
    }

    /// Delete a workspace. Requires `owner`.
    pub async fn delete(&self, caller_account_id: Uuid, workspace_id: Uuid) -> ServiceResult<()> {
        self.require_owner(workspace_id, caller_account_id).await?;
        self.store
            .delete_workspace(workspace_id, caller_account_id)
            .await?;
        Ok(())
    }

    /// Resolve the caller's live role, mapping "no role" to not-found (404).
    async fn require_role(&self, workspace_id: Uuid, account_id: Uuid) -> ServiceResult<Role> {
        self.store
            .role_for(workspace_id, account_id)
            .await?
            .ok_or_else(not_found)
    }

    /// Require the caller to be `owner`: no role is not-found (404), a lesser
    /// role is forbidden (403).
    async fn require_owner(&self, workspace_id: Uuid, account_id: Uuid) -> ServiceResult<()> {
        let role = self.require_role(workspace_id, account_id).await?;
        if role < Role::Owner {
            return Err(ServiceError::Forbidden("owner role required".to_owned()));
        }
        Ok(())
    }
}

/// Reject any caller that is not a user account. Agents can work inside granted
/// workspaces but cannot own/create workspaces.
fn require_user_caller(kind: AccountKind) -> ServiceResult<()> {
    match kind {
        AccountKind::User => Ok(()),
        AccountKind::Agent => Err(ServiceError::Forbidden(
            "only user accounts may create workspaces".to_owned(),
        )),
    }
}

/// The not-found error used to hide workspaces the caller cannot see.
fn not_found() -> ServiceError {
    ServiceError::NotFound("workspace not found".to_owned())
}
