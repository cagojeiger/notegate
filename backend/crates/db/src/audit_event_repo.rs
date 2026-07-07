use crate::map_sqlx_error;
use notegate_core::Result;
use serde_json::Value;
use uuid::Uuid;

pub(crate) const SOURCE_REST: &str = "rest";

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
