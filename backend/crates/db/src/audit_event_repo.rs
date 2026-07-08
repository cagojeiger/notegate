use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_model::AuditEventCursor;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug)]
pub(crate) struct AuditEvent {
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
    event: AuditEvent,
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
    /// `audit_events_owner_time_idx (owner_user_id, occurred_at DESC, id DESC)` order.
    pub async fn list_by_owner(
        &self,
        owner_user_id: Uuid,
        limit: i64,
        cursor: Option<&AuditEventCursor>,
    ) -> Result<Vec<notegate_model::AuditEvent>> {
        let rows = match cursor {
            None => {
                sqlx::query_as::<_, AuditEventRow>(&format!(
                    "SELECT {AUDIT_EVENT_COLUMNS} FROM audit_events \
                     WHERE owner_user_id = $1 \
                     ORDER BY occurred_at DESC, id DESC LIMIT $2"
                ))
                .bind(owner_user_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
            Some(cursor) => {
                sqlx::query_as::<_, AuditEventRow>(&format!(
                    "SELECT {AUDIT_EVENT_COLUMNS} FROM audit_events \
                     WHERE owner_user_id = $1 \
                       AND (occurred_at, id) < ($2, $3) \
                     ORDER BY occurred_at DESC, id DESC LIMIT $4"
                ))
                .bind(owner_user_id)
                .bind(cursor.occurred_at)
                .bind(cursor.id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(map_sqlx_error)?;
        Ok(rows
            .into_iter()
            .map(notegate_model::AuditEvent::from)
            .collect())
    }
}

#[derive(Debug, FromRow)]
struct AuditEventRow {
    id: i64,
    occurred_at: DateTime<Utc>,
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
            occurred_at: row.occurred_at,
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
    "id, occurred_at, actor_account_id, source, op_type, resource_type, resource_id, metadata";
