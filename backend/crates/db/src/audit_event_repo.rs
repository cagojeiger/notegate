//! Audit event persistence: insert plus self-review reads scoped to the
//! caller's own `owner_user_id`.

use crate::event_history_query::{EventCursorPosition, UuidFilter, list_event_rows};
use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_model::AuditEventCursor;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// Write-side row for capture; the read shape is `notegate_model::AuditEvent`.
#[derive(Debug)]
pub(crate) struct NewAuditEvent {
    pub owner_user_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub source: &'static str,
    pub op_type: &'static str,
    pub resource_type: &'static str,
    pub resource_id: Option<Uuid>,
    pub metadata: Value,
}

pub(crate) async fn insert_audit_event(
    tx: &mut sqlx::PgConnection,
    event: NewAuditEvent,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO audit_events \
         (owner_user_id, actor_account_id, source, op_type, resource_type, resource_id, metadata) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(event.owner_user_id)
    .bind(event.actor_account_id)
    .bind(event.source)
    .bind(event.op_type)
    .bind(event.resource_type)
    .bind(event.resource_id)
    .bind(event.metadata)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

/// Read access to `audit_events` for self-review event history: a caller reads
/// only the events scoped to their own `owner_user_id`.
#[derive(Debug, Clone)]
pub struct AuditEventRepo {
    pool: PgPool,
}

impl AuditEventRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// List audit events owned by `owner_user_id`, newest first. Matches the
    /// `audit_events_owner_time_idx (owner_user_id, created_at DESC, id DESC)` order.
    pub async fn list_by_owner(
        &self,
        owner_user_id: Uuid,
        limit: i64,
        cursor: Option<&AuditEventCursor>,
    ) -> Result<Vec<notegate_model::AuditEvent>> {
        let rows = list_event_rows::<AuditEventRow>(
            &self.pool,
            "audit_events",
            AUDIT_EVENT_COLUMNS,
            UuidFilter::new("owner_user_id", owner_user_id),
            None,
            limit,
            cursor.map(|cursor| EventCursorPosition {
                created_at: cursor.created_at,
                id: cursor.id,
            }),
        )
        .await?;
        Ok(rows
            .into_iter()
            .map(notegate_model::AuditEvent::from)
            .collect())
    }
}

#[derive(Debug, FromRow)]
struct AuditEventRow {
    id: i64,
    created_at: DateTime<Utc>,
    actor_account_id: Option<Uuid>,
    source: String,
    op_type: String,
    resource_type: String,
    resource_id: Option<Uuid>,
    metadata: Value,
}

impl From<AuditEventRow> for notegate_model::AuditEvent {
    fn from(row: AuditEventRow) -> Self {
        Self {
            id: row.id,
            created_at: row.created_at,
            actor_account_id: row.actor_account_id,
            source: row.source,
            op_type: row.op_type,
            resource_type: row.resource_type,
            resource_id: row.resource_id,
            metadata: row.metadata,
        }
    }
}

const AUDIT_EVENT_COLUMNS: &str =
    "id, created_at, actor_account_id, source, op_type, resource_type, resource_id, metadata";
