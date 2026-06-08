//! Workspace access: list / grant / revoke, plus the role-check helper used by
//! every later feature.
//!
//! POLICY: managing access is `owner`-only. A caller with no live role sees the
//! workspace as not-found (404); a caller with a lesser role is forbidden (403).
//! A workspace may have at most `WORKSPACE_ACCESS_MAX_ACCOUNTS` live grants,
//! enforced in the grant transaction; revoked/inactive/deleted accounts do not
//! count.

use std::future::Future;

use notegate_core::Result as CoreResult;
use notegate_core::limits;
use notegate_model::{Role, WorkspaceAccess};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::pagination::{clamp_limit, paginate_by_id};

/// Input to list workspace access grants.
#[derive(Debug, Clone, Default)]
pub struct ListAccess {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// A page of access grants.
#[derive(Debug, Clone)]
pub struct AccessPage {
    pub items: Vec<WorkspaceAccess>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// Input to grant (or update) an account's role in a workspace.
#[derive(Debug, Clone)]
pub struct GrantAccess {
    pub workspace_id: Uuid,
    pub account_id: Uuid,
    pub role: Role,
}

/// Persistence for workspace access grants.
pub trait AccessStore: Clone + Send + Sync + 'static {
    /// The caller's live role in a workspace, or `None` if no live grant.
    fn role_for(
        &self,
        workspace_id: Uuid,
        account_id: Uuid,
    ) -> impl Future<Output = CoreResult<Option<Role>>> + Send;

    /// List live access grants for a workspace.
    fn list_access(
        &self,
        workspace_id: Uuid,
    ) -> impl Future<Output = CoreResult<Vec<WorkspaceAccess>>> + Send;

    /// Insert or update a grant, recording the actor in `created_by`. The live
    /// grant cap (`WORKSPACE_ACCESS_MAX_ACCOUNTS`) is enforced in this
    /// transaction; revoked/inactive/deleted accounts do not count.
    fn upsert_access(
        &self,
        command: &GrantAccess,
        created_by: Uuid,
    ) -> impl Future<Output = CoreResult<WorkspaceAccess>> + Send;

    /// Revoke a grant by setting `revoked_at`/`revoked_by`.
    fn revoke_access(
        &self,
        workspace_id: Uuid,
        account_id: Uuid,
        revoked_by: Uuid,
    ) -> impl Future<Output = CoreResult<()>> + Send;
}

/// Workspace access service.
#[derive(Debug, Clone)]
pub struct AccessService<S> {
    store: S,
}

impl<S> AccessService<S>
where
    S: AccessStore,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// List all access grants for a workspace. Requires `owner`.
    pub async fn list(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
    ) -> ServiceResult<Vec<WorkspaceAccess>> {
        self.require_owner(workspace_id, caller_account_id).await?;
        Ok(self.store.list_access(workspace_id).await?)
    }

    /// List access grants for a workspace, paginated with an opaque cursor.
    pub async fn list_page(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        request: ListAccess,
    ) -> ServiceResult<AccessPage> {
        self.require_owner(workspace_id, caller_account_id).await?;
        let limit = clamp_limit(
            request.limit,
            limits::ACCESS_DEFAULT_LIMIT,
            limits::ACCESS_MAX_LIMIT,
        );
        let grants = self.store.list_access(workspace_id).await?;
        let (items, has_more, next_cursor) = paginate_by_id(
            grants,
            |grant| grant.account_id,
            limit,
            request.cursor.as_deref(),
        )?;
        Ok(AccessPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    /// Grant or change an account's role. Requires `owner`.
    pub async fn grant(
        &self,
        caller_account_id: Uuid,
        command: GrantAccess,
    ) -> ServiceResult<WorkspaceAccess> {
        self.require_owner(command.workspace_id, caller_account_id)
            .await?;
        let grants = self.store.list_access(command.workspace_id).await?;
        if would_remove_last_owner(&grants, command.account_id, Some(command.role)) {
            return Err(ServiceError::Conflict(
                "workspace must retain at least one owner".to_owned(),
            ));
        }
        Ok(self
            .store
            .upsert_access(&command, caller_account_id)
            .await?)
    }

    /// Revoke an account's access. Requires `owner`.
    pub async fn revoke(
        &self,
        caller_account_id: Uuid,
        workspace_id: Uuid,
        account_id: Uuid,
    ) -> ServiceResult<()> {
        self.require_owner(workspace_id, caller_account_id).await?;
        let grants = self.store.list_access(workspace_id).await?;
        if would_remove_last_owner(&grants, account_id, None) {
            return Err(ServiceError::Conflict(
                "workspace must retain at least one owner".to_owned(),
            ));
        }
        self.store
            .revoke_access(workspace_id, account_id, caller_account_id)
            .await?;
        Ok(())
    }

    /// Require the caller to be `owner`: no role is not-found (404), a lesser
    /// role is forbidden (403).
    async fn require_owner(&self, workspace_id: Uuid, account_id: Uuid) -> ServiceResult<()> {
        let role = self
            .store
            .role_for(workspace_id, account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("workspace not found".to_owned()))?;
        if role < Role::Owner {
            return Err(ServiceError::Forbidden("owner role required".to_owned()));
        }
        Ok(())
    }
}

/// True when changing `account_id` to `next_role` (or revoking it when
/// `next_role` is `None`) would leave a workspace with no live owner.
fn would_remove_last_owner(
    grants: &[WorkspaceAccess],
    account_id: Uuid,
    next_role: Option<Role>,
) -> bool {
    let target_is_owner = grants
        .iter()
        .any(|grant| grant.account_id == account_id && grant.role == Role::Owner);
    if !target_is_owner {
        return false;
    }

    let target_remains_owner = next_role == Some(Role::Owner);
    if target_remains_owner {
        return false;
    }

    let live_owners = grants
        .iter()
        .filter(|grant| grant.role == Role::Owner)
        .count();
    live_owners <= 1
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
    use crate::cursor;
    use chrono::Utc;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockStore {
        role: Option<Role>,
        access: Vec<WorkspaceAccess>,
        revoked: Arc<Mutex<Vec<(Uuid, Uuid)>>>,
    }

    impl MockStore {
        fn with_role(role: Option<Role>) -> Self {
            Self {
                role,
                access: Vec::new(),
                revoked: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn with_access(role: Option<Role>, access: Vec<WorkspaceAccess>) -> Self {
            Self {
                role,
                access,
                revoked: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl AccessStore for MockStore {
        async fn role_for(&self, _ws: Uuid, _account: Uuid) -> CoreResult<Option<Role>> {
            Ok(self.role)
        }

        async fn list_access(&self, _ws: Uuid) -> CoreResult<Vec<WorkspaceAccess>> {
            Ok(self.access.clone())
        }

        async fn upsert_access(
            &self,
            command: &GrantAccess,
            created_by: Uuid,
        ) -> CoreResult<WorkspaceAccess> {
            Ok(WorkspaceAccess {
                workspace_id: command.workspace_id,
                account_id: command.account_id,
                role: command.role,
                created_by: Some(created_by),
                created_at: Utc::now(),
                revoked_at: None,
                revoked_by: None,
            })
        }

        async fn revoke_access(
            &self,
            workspace_id: Uuid,
            account_id: Uuid,
            _revoked_by: Uuid,
        ) -> CoreResult<()> {
            self.revoked
                .lock()
                .map_err(|_error| notegate_core::Error::internal("lock poisoned"))?
                .push((workspace_id, account_id));
            Ok(())
        }
    }

    #[tokio::test]
    async fn no_role_is_not_found() {
        let service = AccessService::new(MockStore::with_role(None));
        let err = service
            .list(Uuid::new_v4(), Uuid::new_v4())
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[tokio::test]
    async fn editor_cannot_manage_access() {
        let service = AccessService::new(MockStore::with_role(Some(Role::Editor)));
        let err = service
            .grant(
                Uuid::new_v4(),
                GrantAccess {
                    workspace_id: Uuid::new_v4(),
                    account_id: Uuid::new_v4(),
                    role: Role::Viewer,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden(_)));
    }

    #[tokio::test]
    async fn list_page_returns_opaque_cursor() {
        let workspace_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let third = Uuid::new_v4();
        let now = Utc::now();
        let service = AccessService::new(MockStore::with_access(
            Some(Role::Owner),
            vec![
                WorkspaceAccess {
                    workspace_id,
                    account_id: first,
                    role: Role::Viewer,
                    created_by: Some(owner),
                    created_at: now,
                    revoked_at: None,
                    revoked_by: None,
                },
                WorkspaceAccess {
                    workspace_id,
                    account_id: second,
                    role: Role::Editor,
                    created_by: Some(owner),
                    created_at: now,
                    revoked_at: None,
                    revoked_by: None,
                },
                WorkspaceAccess {
                    workspace_id,
                    account_id: third,
                    role: Role::Owner,
                    created_by: Some(owner),
                    created_at: now,
                    revoked_at: None,
                    revoked_by: None,
                },
            ],
        ));

        let first_page = service
            .list_page(
                owner,
                workspace_id,
                ListAccess {
                    limit: Some(2),
                    cursor: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(
            first_page
                .items
                .iter()
                .map(|grant| grant.account_id)
                .collect::<Vec<_>>(),
            vec![first, second]
        );
        assert_eq!(first_page.limit, 2);
        assert!(first_page.has_more);
        let cursor = first_page.next_cursor.expect("next cursor");
        assert_eq!(cursor::decode::<Uuid>(&cursor).unwrap(), second);

        let second_page = service
            .list_page(
                owner,
                workspace_id,
                ListAccess {
                    limit: Some(2),
                    cursor: Some(cursor),
                },
            )
            .await
            .unwrap();

        assert_eq!(
            second_page
                .items
                .iter()
                .map(|grant| grant.account_id)
                .collect::<Vec<_>>(),
            vec![third]
        );
        assert!(!second_page.has_more);
        assert!(second_page.next_cursor.is_none());
    }

    #[tokio::test]
    async fn owner_can_grant_and_revoke() {
        let service = AccessService::new(MockStore::with_role(Some(Role::Owner)));
        assert!(
            service
                .grant(
                    Uuid::new_v4(),
                    GrantAccess {
                        workspace_id: Uuid::new_v4(),
                        account_id: Uuid::new_v4(),
                        role: Role::Editor,
                    },
                )
                .await
                .is_ok()
        );
        assert!(
            service
                .revoke(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4())
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn cannot_remove_the_last_owner() {
        let workspace_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let access = vec![WorkspaceAccess {
            workspace_id,
            account_id: owner,
            role: Role::Owner,
            created_by: Some(owner),
            created_at: Utc::now(),
            revoked_at: None,
            revoked_by: None,
        }];
        let service = AccessService::new(MockStore::with_access(Some(Role::Owner), access));

        let demote = service
            .grant(
                owner,
                GrantAccess {
                    workspace_id,
                    account_id: owner,
                    role: Role::Editor,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(demote, ServiceError::Conflict(_)));

        let revoke = service
            .revoke(owner, workspace_id, owner)
            .await
            .unwrap_err();
        assert!(matches!(revoke, ServiceError::Conflict(_)));
    }
}
