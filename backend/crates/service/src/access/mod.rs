//! Workspace access: list / grant / revoke, plus the role-check helper used by
//! every later feature.
//!
//! POLICY: managing access is lifecycle-owner-only. The owner role is derived
//! from `workspaces.created_by`; access rows store only viewer/editor grants. A
//! caller with no live role sees the workspace as not-found (404); a caller with
//! a lesser role is forbidden (403). A workspace may have at most
//! `WORKSPACE_ACCESS_MAX_ACCOUNTS` live grants, enforced in the grant transaction.

use notegate_core::limits;
use notegate_db::AccessRepo;
pub use notegate_model::{AccessPage, GrantAccess, ListAccess};
use notegate_model::{Role, WorkspaceAccess};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::pagination::{clamp_limit, paginate_by_id};

/// Workspace access service.
#[derive(Debug, Clone)]
pub struct AccessService {
    store: AccessRepo,
}

impl AccessService {
    pub fn new(store: AccessRepo) -> Self {
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
