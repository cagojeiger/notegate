//! Thin `record` helper used by other repositories to log REST-driven
//! mutations to `audit_events` without repeating row-construction boilerplate.

use crate::audit_event_repo::{NewAuditEvent, insert_audit_event};
use notegate_core::Result;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AuditContext {
    actor_account_id: Uuid,
    source: &'static str,
}

impl AuditContext {
    pub(crate) fn rest(actor_account_id: Uuid) -> Self {
        Self {
            actor_account_id,
            source: "rest",
        }
    }
}

pub(crate) async fn record(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    op_type: &'static str,
    resource_type: &'static str,
    resource_id: Option<Uuid>,
    metadata: Value,
) -> Result<()> {
    insert_audit_event(
        tx,
        NewAuditEvent {
            owner_user_id: Some(owner_user_id),
            actor_account_id: Some(ctx.actor_account_id),
            source: ctx.source,
            op_type,
            resource_type,
            resource_id,
            metadata,
        },
    )
    .await
}
