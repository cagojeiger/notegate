//! Typed constructors for `audit_events` rows: one function per audit event
//! kind builds its `op_type` + `metadata` (see `docs/spec/event-logging.md`
//! for the allowlist), so the account/space/agent/connection repositories
//! never inline strings or `json!` payloads — they only call a constructor
//! here.
//!
//! To add a new event type: add a `..._payload` function that returns the
//! `(op_type, metadata)` pair, a thin `pub(crate) async fn` wrapper that
//! forwards it to `event`, and a unit test asserting the payload shape.

use crate::audit_event_repo::{NewAuditEvent, insert_audit_event};
use notegate_core::Result;
use serde_json::{Value, json};
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

async fn event(
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

fn space_created_payload() -> (&'static str, Value) {
    ("space.create", json!({}))
}

pub(crate) async fn space_created(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    space_id: Uuid,
) -> Result<()> {
    let (op_type, metadata) = space_created_payload();
    event(
        tx,
        ctx,
        owner_user_id,
        op_type,
        "space",
        Some(space_id),
        metadata,
    )
    .await
}

fn space_updated_payload(changed_fields: &[&str]) -> (&'static str, Value) {
    ("space.update", json!({ "changed_fields": changed_fields }))
}

pub(crate) async fn space_updated(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    space_id: Uuid,
    changed_fields: &[&str],
) -> Result<()> {
    let (op_type, metadata) = space_updated_payload(changed_fields);
    event(
        tx,
        ctx,
        owner_user_id,
        op_type,
        "space",
        Some(space_id),
        metadata,
    )
    .await
}

fn space_deleted_payload() -> (&'static str, Value) {
    ("space.delete", json!({}))
}

pub(crate) async fn space_deleted(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    space_id: Uuid,
) -> Result<()> {
    let (op_type, metadata) = space_deleted_payload();
    event(
        tx,
        ctx,
        owner_user_id,
        op_type,
        "space",
        Some(space_id),
        metadata,
    )
    .await
}

fn agent_created_payload() -> (&'static str, Value) {
    ("agent.create", json!({}))
}

pub(crate) async fn agent_created(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    agent_id: Uuid,
) -> Result<()> {
    let (op_type, metadata) = agent_created_payload();
    event(
        tx,
        ctx,
        owner_user_id,
        op_type,
        "agent",
        Some(agent_id),
        metadata,
    )
    .await
}

fn agent_deleted_payload(
    revoked_agent_keys: u64,
    disconnected_connections: u64,
) -> (&'static str, Value) {
    (
        "agent.delete",
        json!({
            "revoked_agent_keys": revoked_agent_keys,
            "disconnected_connections": disconnected_connections,
        }),
    )
}

pub(crate) async fn agent_deleted(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    agent_id: Uuid,
    revoked_agent_keys: u64,
    disconnected_connections: u64,
) -> Result<()> {
    let (op_type, metadata) = agent_deleted_payload(revoked_agent_keys, disconnected_connections);
    event(
        tx,
        ctx,
        owner_user_id,
        op_type,
        "agent",
        Some(agent_id),
        metadata,
    )
    .await
}

/// Which account kind an API key belongs to. Selects the `user_key.*` vs
/// `agent_key.*` op_type family for the same create/rotate/revoke shapes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ApiKeyOwnerKind {
    User,
    Agent,
}

impl ApiKeyOwnerKind {
    fn create_op_type(self) -> &'static str {
        match self {
            ApiKeyOwnerKind::User => "user_key.create",
            ApiKeyOwnerKind::Agent => "agent_key.create",
        }
    }

    fn rotate_op_type(self) -> &'static str {
        match self {
            ApiKeyOwnerKind::User => "user_key.rotate",
            ApiKeyOwnerKind::Agent => "agent_key.rotate",
        }
    }

    fn revoke_op_type(self) -> &'static str {
        match self {
            ApiKeyOwnerKind::User => "user_key.revoke",
            ApiKeyOwnerKind::Agent => "agent_key.revoke",
        }
    }
}

fn api_key_created_payload(kind: ApiKeyOwnerKind) -> (&'static str, Value) {
    (kind.create_op_type(), json!({}))
}

pub(crate) async fn api_key_created(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    kind: ApiKeyOwnerKind,
    key_id: Uuid,
) -> Result<()> {
    let (op_type, metadata) = api_key_created_payload(kind);
    event(
        tx,
        ctx,
        owner_user_id,
        op_type,
        "api_key",
        Some(key_id),
        metadata,
    )
    .await
}

fn api_key_rotated_payload(kind: ApiKeyOwnerKind, created_key_id: Uuid) -> (&'static str, Value) {
    (
        kind.rotate_op_type(),
        json!({ "created_key_id": created_key_id }),
    )
}

pub(crate) async fn api_key_rotated(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    kind: ApiKeyOwnerKind,
    old_key_id: Uuid,
    created_key_id: Uuid,
) -> Result<()> {
    let (op_type, metadata) = api_key_rotated_payload(kind, created_key_id);
    event(
        tx,
        ctx,
        owner_user_id,
        op_type,
        "api_key",
        Some(old_key_id),
        metadata,
    )
    .await
}

fn api_key_revoked_payload(kind: ApiKeyOwnerKind, reason: Option<&str>) -> (&'static str, Value) {
    let metadata = reason
        .map(|reason| json!({ "reason": reason }))
        .unwrap_or_else(|| json!({}));
    (kind.revoke_op_type(), metadata)
}

pub(crate) async fn api_key_revoked(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    kind: ApiKeyOwnerKind,
    key_id: Uuid,
    reason: Option<&str>,
) -> Result<()> {
    let (op_type, metadata) = api_key_revoked_payload(kind, reason);
    event(
        tx,
        ctx,
        owner_user_id,
        op_type,
        "api_key",
        Some(key_id),
        metadata,
    )
    .await
}

fn connection_upserted_payload(agent_id: Uuid, permission: &str) -> (&'static str, Value) {
    (
        "connection.upsert",
        json!({ "agent_id": agent_id, "permission": permission }),
    )
}

pub(crate) async fn connection_upserted(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    space_id: Uuid,
    agent_id: Uuid,
    permission: &str,
) -> Result<()> {
    let (op_type, metadata) = connection_upserted_payload(agent_id, permission);
    event(
        tx,
        ctx,
        owner_user_id,
        op_type,
        "space",
        Some(space_id),
        metadata,
    )
    .await
}

fn connection_disconnected_payload(agent_id: Uuid) -> (&'static str, Value) {
    ("connection.disconnect", json!({ "agent_id": agent_id }))
}

pub(crate) async fn connection_disconnected(
    tx: &mut PgConnection,
    ctx: AuditContext,
    owner_user_id: Uuid,
    space_id: Uuid,
    agent_id: Uuid,
) -> Result<()> {
    let (op_type, metadata) = connection_disconnected_payload(agent_id);
    event(
        tx,
        ctx,
        owner_user_id,
        op_type,
        "space",
        Some(space_id),
        metadata,
    )
    .await
}

/// Row-affected counts from the account-delete cascade (ADR 0004), recorded
/// on the `account.delete` audit event.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AccountDeleteCounts {
    pub(crate) deactivated_agents: u64,
    pub(crate) revoked_api_keys: u64,
    pub(crate) revoked_browser_sessions: u64,
    pub(crate) disconnected_connections: u64,
}

fn account_deleted_payload(counts: AccountDeleteCounts) -> (&'static str, Value) {
    (
        "account.delete",
        json!({
            "deactivated_agents": counts.deactivated_agents,
            "revoked_api_keys": counts.revoked_api_keys,
            "revoked_browser_sessions": counts.revoked_browser_sessions,
            "disconnected_connections": counts.disconnected_connections,
        }),
    )
}

pub(crate) async fn account_deleted(
    tx: &mut PgConnection,
    ctx: AuditContext,
    account_id: Uuid,
    counts: AccountDeleteCounts,
) -> Result<()> {
    let (op_type, metadata) = account_deleted_payload(counts);
    event(
        tx,
        ctx,
        account_id,
        op_type,
        "account",
        Some(account_id),
        metadata,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_created_builds_expected_payload() {
        let (op_type, metadata) = space_created_payload();
        assert_eq!(op_type, "space.create");
        assert_eq!(metadata, json!({}));
    }

    #[test]
    fn space_updated_builds_expected_payload() {
        let (op_type, metadata) = space_updated_payload(&["name", "sort_order"]);
        assert_eq!(op_type, "space.update");
        assert_eq!(
            metadata,
            json!({ "changed_fields": ["name", "sort_order"] })
        );
    }

    #[test]
    fn space_deleted_builds_expected_payload() {
        let (op_type, metadata) = space_deleted_payload();
        assert_eq!(op_type, "space.delete");
        assert_eq!(metadata, json!({}));
    }

    #[test]
    fn agent_created_builds_expected_payload() {
        let (op_type, metadata) = agent_created_payload();
        assert_eq!(op_type, "agent.create");
        assert_eq!(metadata, json!({}));
    }

    #[test]
    fn agent_deleted_builds_expected_payload() {
        let (op_type, metadata) = agent_deleted_payload(2, 1);
        assert_eq!(op_type, "agent.delete");
        assert_eq!(
            metadata,
            json!({ "revoked_agent_keys": 2, "disconnected_connections": 1 })
        );
    }

    #[test]
    fn api_key_created_uses_owner_kind_op_type() {
        let (user_op_type, user_metadata) = api_key_created_payload(ApiKeyOwnerKind::User);
        assert_eq!(user_op_type, "user_key.create");
        assert_eq!(user_metadata, json!({}));

        let (agent_op_type, agent_metadata) = api_key_created_payload(ApiKeyOwnerKind::Agent);
        assert_eq!(agent_op_type, "agent_key.create");
        assert_eq!(agent_metadata, json!({}));
    }

    #[test]
    fn api_key_rotated_uses_owner_kind_op_type_and_created_key_id() {
        let created_key_id = Uuid::new_v4();
        let (op_type, metadata) = api_key_rotated_payload(ApiKeyOwnerKind::User, created_key_id);
        assert_eq!(op_type, "user_key.rotate");
        assert_eq!(metadata, json!({ "created_key_id": created_key_id }));

        let (op_type, metadata) = api_key_rotated_payload(ApiKeyOwnerKind::Agent, created_key_id);
        assert_eq!(op_type, "agent_key.rotate");
        assert_eq!(metadata, json!({ "created_key_id": created_key_id }));
    }

    #[test]
    fn api_key_revoked_includes_reason_when_present() {
        let (op_type, metadata) = api_key_revoked_payload(ApiKeyOwnerKind::User, Some("manual"));
        assert_eq!(op_type, "user_key.revoke");
        assert_eq!(metadata, json!({ "reason": "manual" }));
    }

    #[test]
    fn api_key_revoked_omits_reason_when_absent() {
        let (op_type, metadata) = api_key_revoked_payload(ApiKeyOwnerKind::Agent, None);
        assert_eq!(op_type, "agent_key.revoke");
        assert_eq!(metadata, json!({}));
    }

    #[test]
    fn connection_upserted_builds_expected_payload() {
        let agent_id = Uuid::new_v4();
        let (op_type, metadata) = connection_upserted_payload(agent_id, "write");
        assert_eq!(op_type, "connection.upsert");
        assert_eq!(
            metadata,
            json!({ "agent_id": agent_id, "permission": "write" })
        );
    }

    #[test]
    fn connection_disconnected_builds_expected_payload() {
        let agent_id = Uuid::new_v4();
        let (op_type, metadata) = connection_disconnected_payload(agent_id);
        assert_eq!(op_type, "connection.disconnect");
        assert_eq!(metadata, json!({ "agent_id": agent_id }));
    }

    #[test]
    fn account_deleted_builds_expected_payload() {
        let counts = AccountDeleteCounts {
            deactivated_agents: 1,
            revoked_api_keys: 2,
            revoked_browser_sessions: 1,
            disconnected_connections: 1,
        };
        let (op_type, metadata) = account_deleted_payload(counts);
        assert_eq!(op_type, "account.delete");
        assert_eq!(
            metadata,
            json!({
                "deactivated_agents": 1,
                "revoked_api_keys": 2,
                "revoked_browser_sessions": 1,
                "disconnected_connections": 1,
            })
        );
    }
}
