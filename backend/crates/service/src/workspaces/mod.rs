//! Workspace lifecycle: create / get / rename / delete.
//!
//! POLICY: workspace access is the authorization boundary. `create` is user-only,
//! grants the creator `owner`, and relies on the DB trigger to materialize the
//! canonical root node; a user owner account may own at most
//! [`limits::OWNED_WORKSPACES_MAX`] workspaces (enforced in the create
//! transaction). `get` is visible to any live role; `rename`/`delete` require
//! `owner`. A workspace the caller cannot see (no live role) is reported as
//! not-found so the api returns `404`.

use std::future::Future;

use chrono::{DateTime, Utc};
use notegate_core::Result as CoreResult;
use notegate_core::limits;
use notegate_core::validation::validate_workspace_name;
use notegate_model::{AccountKind, Role, Workspace};
use uuid::Uuid;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::pagination::clamp_limit;

/// Input to create a workspace. The owner is the authenticated user caller and
/// is passed separately to the service/store boundary.
#[derive(Debug, Clone)]
pub struct CreateWorkspace {
    pub name: String,
}

/// Input to rename a workspace.
#[derive(Debug, Clone)]
pub struct RenameWorkspace {
    pub workspace_id: Uuid,
    pub new_name: String,
}

/// Input to list visible workspaces.
#[derive(Debug, Clone, Default)]
pub struct ListWorkspaces {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// Keyset cursor for workspace list order `(created_at, id)`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

/// A workspace plus the caller's role and derived root node id, returned by
/// `get` and `list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceView {
    pub workspace: Workspace,
    pub role: Role,
    pub root_node_id: Uuid,
}

/// A page of workspace views.
#[derive(Debug, Clone)]
pub struct WorkspacePage {
    pub items: Vec<WorkspaceView>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// Persistence for workspaces and the per-workspace role lookup.
pub trait WorkspaceStore: Clone + Send + Sync + 'static {
    /// The caller's live role in a workspace, or `None` if no live grant.
    fn role_for(
        &self,
        workspace_id: Uuid,
        account_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<Role>>> + Send;

    /// Insert a workspace owned by the user `owner_account_id`, relying on the DB trigger
    /// to create the root node, and grant that owner `owner` in the same
    /// transaction. The owner-quota ([`limits::OWNED_WORKSPACES_MAX`]) is
    /// enforced in that transaction.
    fn create_workspace(
        &self,
        owner_account_id: Uuid,
        command: &CreateWorkspace,
    ) -> impl Future<Output = CoreResult<Workspace>> + Send;

    /// Load a workspace by id, regardless of caller access.
    fn find_workspace(
        &self,
        workspace_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<Workspace>>> + Send;

    /// Load one workspace view by id if the account has a live role.
    fn find_workspace_view_for(
        &self,
        account_id: Uuid,
        workspace_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<WorkspaceView>>> + Send;

    /// List workspace views matching an exact name where the account has a live
    /// role. Used by MCP name selection; callers usually request 2 rows to
    /// detect ambiguity without scanning every accessible workspace.
    fn list_workspace_views_by_name_for(
        &self,
        account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> impl Future<Output = CoreResult<Vec<WorkspaceView>>> + Send;

    /// List workspace views where the account has a live role. Returns up to
    /// `limit` rows in `(created_at, id)` order.
    fn list_workspace_views_for(
        &self,
        account_id: Uuid,
        limit: i64,
        cursor: Option<&WorkspaceCursor>,
    ) -> impl Future<Output = CoreResult<Vec<WorkspaceView>>> + Send;

    /// Rename a workspace. Workspaces carry no `updated_by`, so this updates
    /// only `name` and `updated_at`.
    fn rename_workspace(
        &self,
        workspace_id: Uuid,
        new_name: &str,
    ) -> impl Future<Output = CoreResult<Workspace>> + Send;

    /// Delete a workspace; the DB cascade removes access rows, nodes, documents.
    fn delete_workspace(&self, workspace_id: Uuid) -> impl Future<Output = CoreResult<()>> + Send;

    /// The id of a workspace's canonical root node (`parent_id IS NULL`).
    fn root_node_id(
        &self,
        workspace_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<Uuid>>> + Send;
}

/// Workspace lifecycle service.
#[derive(Debug, Clone)]
pub struct WorkspaceService<S> {
    store: S,
}

impl<S> WorkspaceService<S>
where
    S: WorkspaceStore,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Create a workspace owned by the authenticated user caller, granting that
    /// user account `owner`. Enforces the owner quota and a clean name conflict.
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
        self.store.delete_workspace(workspace_id).await?;
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use super::*;
    use chrono::Utc;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockStore {
        role: Option<Role>,
        deleted: Arc<Mutex<Vec<Uuid>>>,
    }

    impl MockStore {
        fn with_role(role: Option<Role>) -> Self {
            Self {
                role,
                deleted: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    fn sample_workspace() -> Workspace {
        Workspace {
            id: Uuid::new_v4(),
            owner_account_id: Uuid::new_v4(),
            name: "personal".to_owned(),
            created_by: Uuid::new_v4(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    impl WorkspaceStore for MockStore {
        async fn role_for(&self, _ws: Uuid, _account: Uuid) -> CoreResult<Option<Role>> {
            Ok(self.role)
        }

        async fn create_workspace(
            &self,
            owner_account_id: Uuid,
            command: &CreateWorkspace,
        ) -> CoreResult<Workspace> {
            Ok(Workspace {
                id: Uuid::new_v4(),
                owner_account_id,
                name: command.name.clone(),
                created_by: owner_account_id,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        }

        async fn find_workspace(&self, _ws: Uuid) -> CoreResult<Option<Workspace>> {
            Ok(Some(sample_workspace()))
        }

        async fn find_workspace_view_for(
            &self,
            _account: Uuid,
            _workspace_id: Uuid,
        ) -> CoreResult<Option<WorkspaceView>> {
            Ok(Some(WorkspaceView {
                workspace: sample_workspace(),
                role: Role::Owner,
                root_node_id: Uuid::new_v4(),
            }))
        }

        async fn list_workspace_views_by_name_for(
            &self,
            _account: Uuid,
            _name: &str,
            limit: i64,
        ) -> CoreResult<Vec<WorkspaceView>> {
            Ok(vec![WorkspaceView {
                workspace: sample_workspace(),
                role: Role::Owner,
                root_node_id: Uuid::new_v4(),
            }]
            .into_iter()
            .take(limit as usize)
            .collect())
        }

        async fn list_workspace_views_for(
            &self,
            _account: Uuid,
            limit: i64,
            _cursor: Option<&WorkspaceCursor>,
        ) -> CoreResult<Vec<WorkspaceView>> {
            Ok(vec![WorkspaceView {
                workspace: sample_workspace(),
                role: Role::Owner,
                root_node_id: Uuid::new_v4(),
            }]
            .into_iter()
            .take(limit as usize)
            .collect())
        }

        async fn rename_workspace(&self, _ws: Uuid, new_name: &str) -> CoreResult<Workspace> {
            let mut workspace = sample_workspace();
            workspace.name = new_name.to_owned();
            Ok(workspace)
        }

        async fn delete_workspace(&self, ws: Uuid) -> CoreResult<()> {
            self.deleted
                .lock()
                .map_err(|_error| notegate_core::Error::internal("lock poisoned"))?
                .push(ws);
            Ok(())
        }

        async fn root_node_id(&self, _ws: Uuid) -> CoreResult<Option<Uuid>> {
            Ok(Some(Uuid::new_v4()))
        }
    }

    #[tokio::test]
    async fn create_returns_owner_view() {
        let service = WorkspaceService::new(MockStore::with_role(None));
        let view = service
            .create(
                AccountKind::User,
                Uuid::new_v4(),
                CreateWorkspace {
                    name: "notes".to_owned(),
                },
            )
            .await
            .unwrap();
        assert_eq!(view.role, Role::Owner);
        assert_eq!(view.workspace.name, "notes");
    }

    #[tokio::test]
    async fn agent_cannot_create_workspace() {
        let service = WorkspaceService::new(MockStore::with_role(None));
        let err = service
            .create(
                AccountKind::Agent,
                Uuid::new_v4(),
                CreateWorkspace {
                    name: "notes".to_owned(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden(_)));
    }

    #[tokio::test]
    async fn create_rejects_invalid_name() {
        let service = WorkspaceService::new(MockStore::with_role(None));
        let err = service
            .create(
                AccountKind::User,
                Uuid::new_v4(),
                CreateWorkspace {
                    name: ".hidden".to_owned(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn get_without_role_is_not_found() {
        let service = WorkspaceService::new(MockStore::with_role(None));
        let err = service
            .get(Uuid::new_v4(), Uuid::new_v4())
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[tokio::test]
    async fn viewer_cannot_rename() {
        let service = WorkspaceService::new(MockStore::with_role(Some(Role::Viewer)));
        let err = service
            .rename(
                Uuid::new_v4(),
                RenameWorkspace {
                    workspace_id: Uuid::new_v4(),
                    new_name: "renamed".to_owned(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden(_)));
    }

    #[tokio::test]
    async fn editor_cannot_delete_but_owner_can() {
        let editor = WorkspaceService::new(MockStore::with_role(Some(Role::Editor)));
        let err = editor
            .delete(Uuid::new_v4(), Uuid::new_v4())
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden(_)));

        let owner = WorkspaceService::new(MockStore::with_role(Some(Role::Owner)));
        assert!(owner.delete(Uuid::new_v4(), Uuid::new_v4()).await.is_ok());
    }
}
