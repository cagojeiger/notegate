use std::collections::HashMap;

use chrono::{DateTime, Utc};
use notegate_model::{AccountRef as ModelAccountRef, AuditEvent};
use notegate_service::files::FileChangeEvent;
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::AccountRef;

/// Audit event history entry returned by `GET /api/v1/me/audit-events`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AuditEventOut {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub actor_account_id: Option<Uuid>,
    pub actor: Option<AccountRef>,
    pub source: String,
    pub op_type: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub metadata: Value,
}

impl AuditEventOut {
    pub(crate) fn from_event(event: &AuditEvent, refs: &HashMap<Uuid, ModelAccountRef>) -> Self {
        Self {
            id: event.id,
            created_at: event.created_at,
            actor_account_id: event.actor_account_id,
            actor: event
                .actor_account_id
                .and_then(|id| refs.get(&id).map(AccountRef::from)),
            source: event.source.clone(),
            op_type: event.op_type.clone(),
            resource_type: event.resource_type.clone(),
            resource_id: event.resource_id,
            metadata: event.metadata.clone(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AuditEventListResponse {
    pub events: Vec<AuditEventOut>,
    pub page: crate::page::Page,
}

/// File change event history entry returned by `GET /api/v1/spaces/{space_id}/file-change-events`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileChangeEventOut {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub space_id: Uuid,
    pub node_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub actor: Option<AccountRef>,
    pub op_type: String,
    pub metadata: Value,
}

impl FileChangeEventOut {
    pub(crate) fn from_event(
        event: &FileChangeEvent,
        refs: &HashMap<Uuid, ModelAccountRef>,
    ) -> Self {
        Self {
            id: event.id,
            created_at: event.created_at,
            space_id: event.space_id,
            node_id: event.node_id,
            actor_account_id: event.actor_account_id,
            actor: event
                .actor_account_id
                .and_then(|id| refs.get(&id).map(AccountRef::from)),
            op_type: event.op_type.clone(),
            metadata: event.metadata.clone(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileChangeEventListResponse {
    pub events: Vec<FileChangeEventOut>,
    pub page: crate::page::Page,
}
