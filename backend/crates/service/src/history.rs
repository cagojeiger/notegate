//! User-owned history queries, separate from account lifecycle mutations.

use notegate_db::{AuditEventRepo, BackgroundJobRepo, ChangeHistoryRepo, CommandInvocationRepo};
use notegate_model::account::AccountKind;
use notegate_model::{
    AuditEventPage, BackgroundJobDetail, BackgroundJobPage, CommandInvocationPage, ListAuditEvents,
    ListBackgroundJobs, ListCommandInvocations,
};
use uuid::Uuid;

use crate::audit_events::list_audit_event_page;
use crate::background_jobs::{get_background_job, list_background_job_page};
use crate::command_invocations::list_command_invocation_page;
use crate::{ServiceError, ServiceResult};

#[derive(Debug, Clone)]
pub struct HistoryService {
    audit_events: AuditEventRepo,
    command_invocations: CommandInvocationRepo,
    background_jobs: BackgroundJobRepo,
    changes: ChangeHistoryRepo,
}

impl HistoryService {
    pub fn new(
        audit_events: AuditEventRepo,
        command_invocations: CommandInvocationRepo,
        background_jobs: BackgroundJobRepo,
        changes: ChangeHistoryRepo,
    ) -> Self {
        Self {
            audit_events,
            command_invocations,
            background_jobs,
            changes,
        }
    }

    /// Owned metadata snapshots remain accessible after the Space is removed.
    /// This does not authorize any document/revision body access.
    pub async fn list_file_changes(
        &self,
        caller_kind: AccountKind,
        owner: Uuid,
        space: Option<Uuid>,
        limit: Option<i64>,
        raw_cursor: Option<String>,
    ) -> ServiceResult<notegate_model::FileChangeEventPage> {
        require_user(caller_kind)?;
        let (items, limit, has_more, next_cursor) = crate::pagination::paginate_keyset(
            limit,
            notegate_core::limits::FILE_CHANGE_EVENTS_DEFAULT_LIMIT,
            notegate_core::limits::FILE_CHANGE_EVENTS_MAX_LIMIT,
            raw_cursor.as_deref(),
            |limit, cursor: Option<notegate_model::FileChangeEventCursor>| async move {
                Ok(self
                    .changes
                    .list_by_owner(owner, space, limit, cursor.as_ref())
                    .await?)
            },
            |event| notegate_model::FileChangeEventCursor {
                created_at: event.created_at,
                id: event.id,
            },
        )
        .await?;
        Ok(notegate_model::FileChangeEventPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
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

    /// List external command calls owned by the current user. User callers only.
    pub async fn list_command_invocations(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        request: ListCommandInvocations,
    ) -> ServiceResult<CommandInvocationPage> {
        require_user(caller_kind)?;
        list_command_invocation_page(&self.command_invocations, caller_account_id, request).await
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
