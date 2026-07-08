//! Audit event history service: self-review event queries for the calling user.

use notegate_core::limits;
use notegate_db::AuditEventRepo;
use notegate_model::{AuditEventCursor, AuditEventPage, ListAuditEvents};
use uuid::Uuid;

use crate::pagination::clamp_limit;
use crate::{ServiceError, ServiceResult, cursor};

pub async fn list_audit_event_page(
    audit_events: &AuditEventRepo,
    owner_user_id: Uuid,
    request: ListAuditEvents,
) -> ServiceResult<AuditEventPage> {
    let limit = clamp_limit(
        request.limit,
        limits::AUDIT_EVENTS_DEFAULT_LIMIT,
        limits::AUDIT_EVENTS_MAX_LIMIT,
    );
    let cursor = match request.cursor.as_deref() {
        None => None,
        Some(raw) => Some(
            cursor::decode::<AuditEventCursor>(raw)
                .map_err(|_error| ServiceError::InvalidInput("invalid cursor".to_owned()))?,
        ),
    };

    let mut items = audit_events
        .list_by_owner(owner_user_id, limit + 1, cursor.as_ref())
        .await?;
    let has_more = items.len() as i64 > limit;
    items.truncate(limit as usize);
    let next_cursor = if has_more {
        items
            .last()
            .map(|event| AuditEventCursor {
                occurred_at: event.occurred_at,
                id: event.id,
            })
            .map(|cursor| cursor::encode(&cursor))
            .transpose()
            .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
    } else {
        None
    };

    Ok(AuditEventPage {
        items,
        limit,
        has_more,
        next_cursor,
    })
}
