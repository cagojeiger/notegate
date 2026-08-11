//! Account lifecycle operations for the current caller.

use notegate_db::{AccountRepo, AuditEventRepo, BackgroundJobRepo, McpInvocationRepo};
use notegate_model::account::AccountKind;
use notegate_model::{
    AuditEventPage, BackgroundJobDetail, BackgroundJobPage, ListAuditEvents, ListBackgroundJobs,
    ListMcpInvocations, McpInvocationPage,
};
use uuid::Uuid;

use crate::audit_events::list_audit_event_page;
use crate::background_jobs::{get_background_job, list_background_job_page};
use crate::mcp_invocations::list_mcp_invocation_page;
use crate::{ServiceError, ServiceResult};

#[derive(Debug, Clone)]
pub struct AccountService {
    store: AccountRepo,
    audit_events: AuditEventRepo,
    mcp_invocations: McpInvocationRepo,
    background_jobs: BackgroundJobRepo,
}

impl AccountService {
    pub fn new(
        store: AccountRepo,
        audit_events: AuditEventRepo,
        mcp_invocations: McpInvocationRepo,
        background_jobs: BackgroundJobRepo,
    ) -> Self {
        Self {
            store,
            audit_events,
            mcp_invocations,
            background_jobs,
        }
    }

    /// Soft-delete the current user account (ADR 0004). PII and the provider-sub
    /// tombstone are retained until the purge run anonymizes them after the retention
    /// window; re-login during that window is rejected, so a returning sub is never
    /// duplicated.
    ///
    /// Agent callers cannot delete accounts through this user lifecycle endpoint.
    pub async fn delete_me(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
    ) -> ServiceResult<()> {
        if caller_kind != AccountKind::User {
            return Err(ServiceError::Forbidden(
                "only user accounts may delete themselves".to_owned(),
            ));
        }
        // ADR 0004: spaces are cleaned up manually. Block deletion while the caller
        // still owns any live space — they must delete it first.
        let sole_owned = self
            .store
            .count_sole_owned_spaces(caller_account_id)
            .await?;
        if sole_owned > 0 {
            return Err(ServiceError::Conflict(format!(
                "delete your {sole_owned} owned space(s) before deleting your account"
            )));
        }
        Ok(self
            .store
            .soft_delete_user(caller_account_id, caller_account_id)
            .await?)
    }

    /// List the caller's own audit event history (self-review). User callers only.
    pub async fn list_audit_events(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        request: ListAuditEvents,
    ) -> ServiceResult<AuditEventPage> {
        require_user(caller_kind)?;
        list_audit_event_page(&self.audit_events, caller_account_id, request).await
    }

    /// List MCP calls owned by the current user. User callers only.
    pub async fn list_mcp_invocations(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        request: ListMcpInvocations,
    ) -> ServiceResult<McpInvocationPage> {
        require_user(caller_kind)?;
        list_mcp_invocation_page(&self.mcp_invocations, caller_account_id, request).await
    }

    /// List background jobs recorded in the current user's account-scoped history.
    pub async fn list_background_jobs(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        request: ListBackgroundJobs,
    ) -> ServiceResult<BackgroundJobPage> {
        require_user(caller_kind)?;
        list_background_job_page(&self.background_jobs, caller_account_id, request).await
    }

    /// Get one owned queue job with its attempt history.
    pub async fn get_background_job(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        job_id: Uuid,
    ) -> ServiceResult<BackgroundJobDetail> {
        require_user(caller_kind)?;
        get_background_job(&self.background_jobs, caller_account_id, job_id).await
    }
}

fn require_user(kind: AccountKind) -> ServiceResult<()> {
    if kind == AccountKind::User {
        Ok(())
    } else {
        Err(ServiceError::Forbidden(
            "only user accounts may access this endpoint".to_owned(),
        ))
    }
}
