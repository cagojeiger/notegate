//! Audit event history service: self-review event queries for the calling user.

use notegate_core::limits;
use notegate_db::AuditEventRepo;
use notegate_model::{AuditEventCursor, AuditEventPage, ListAuditEvents};
use uuid::Uuid;

use crate::ServiceResult;
use crate::pagination::paginate_keyset;

pub async fn list_audit_event_page(
    audit_events: &AuditEventRepo,
    owner_user_id: Uuid,
    request: ListAuditEvents,
) -> ServiceResult<AuditEventPage> {
    let (items, limit, has_more, next_cursor) = paginate_keyset(
        request.limit,
        limits::AUDIT_EVENTS_DEFAULT_LIMIT,
        limits::AUDIT_EVENTS_MAX_LIMIT,
        request.cursor.as_deref(),
        |limit, cursor: Option<AuditEventCursor>| async move {
            Ok(audit_events
                .list_by_owner(owner_user_id, limit, cursor.as_ref())
                .await?)
        },
        |event| AuditEventCursor {
            created_at: event.created_at,
            id: event.id,
        },
    )
    .await?;

    Ok(AuditEventPage {
        items,
        limit,
        has_more,
        next_cursor,
    })
}
