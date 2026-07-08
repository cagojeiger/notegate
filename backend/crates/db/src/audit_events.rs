use crate::audit_event_repo::NewAuditEvent;
use serde_json::json;
use uuid::Uuid;

/// Audited management mutations are reachable only through REST today, so
/// `Rest` is the only variant; `mcp`/`system` in the DB CHECK are reserved for
/// future streams and workers.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AuditSource {
    Rest,
}

impl AuditSource {
    fn as_str(self) -> &'static str {
        match self {
            AuditSource::Rest => "rest",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AuditContext {
    source: AuditSource,
    actor_account_id: Uuid,
}

impl AuditContext {
    pub(crate) fn rest(actor_account_id: Uuid) -> Self {
        Self {
            source: AuditSource::Rest,
            actor_account_id,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ApiKeyAuditKind {
    User,
    Agent,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AccountDeleteCounts {
    pub deactivated_agents: u64,
    pub revoked_api_keys: u64,
    pub revoked_browser_sessions: u64,
    pub disconnected_connections: u64,
}

pub(crate) fn space_created(
    ctx: AuditContext,
    owner_user_id: Uuid,
    space_id: Uuid,
) -> NewAuditEvent {
    event(
        ctx,
        owner_user_id,
        "space.create",
        "space",
        Some(space_id),
        json!({}),
    )
}

pub(crate) fn space_updated(
    ctx: AuditContext,
    owner_user_id: Uuid,
    space_id: Uuid,
    changed_fields: Vec<&'static str>,
) -> NewAuditEvent {
    event(
        ctx,
        owner_user_id,
        "space.update",
        "space",
        Some(space_id),
        json!({ "changed_fields": changed_fields }),
    )
}

pub(crate) fn space_deleted(
    ctx: AuditContext,
    owner_user_id: Uuid,
    space_id: Uuid,
) -> NewAuditEvent {
    event(
        ctx,
        owner_user_id,
        "space.delete",
        "space",
        Some(space_id),
        json!({}),
    )
}

pub(crate) fn agent_created(
    ctx: AuditContext,
    owner_user_id: Uuid,
    agent_id: Uuid,
) -> NewAuditEvent {
    event(
        ctx,
        owner_user_id,
        "agent.create",
        "agent",
        Some(agent_id),
        json!({}),
    )
}

pub(crate) fn agent_deleted(
    ctx: AuditContext,
    owner_user_id: Uuid,
    agent_id: Uuid,
    revoked_agent_keys: u64,
    disconnected_connections: u64,
) -> NewAuditEvent {
    event(
        ctx,
        owner_user_id,
        "agent.delete",
        "agent",
        Some(agent_id),
        json!({
            "revoked_agent_keys": revoked_agent_keys,
            "disconnected_connections": disconnected_connections,
        }),
    )
}

pub(crate) fn connection_upserted(
    ctx: AuditContext,
    owner_user_id: Uuid,
    space_id: Uuid,
    agent_id: Uuid,
    permission: &str,
) -> NewAuditEvent {
    event(
        ctx,
        owner_user_id,
        "connection.upsert",
        "space",
        Some(space_id),
        json!({
            "agent_id": agent_id,
            "permission": permission,
        }),
    )
}

pub(crate) fn connection_disconnected(
    ctx: AuditContext,
    owner_user_id: Uuid,
    space_id: Uuid,
    agent_id: Uuid,
) -> NewAuditEvent {
    event(
        ctx,
        owner_user_id,
        "connection.disconnect",
        "space",
        Some(space_id),
        json!({ "agent_id": agent_id }),
    )
}

pub(crate) fn api_key_created(
    ctx: AuditContext,
    owner_user_id: Uuid,
    key_id: Uuid,
    kind: ApiKeyAuditKind,
) -> NewAuditEvent {
    event(
        ctx,
        owner_user_id,
        kind.create_op_type(),
        "api_key",
        Some(key_id),
        json!({}),
    )
}

pub(crate) fn api_key_rotated(
    ctx: AuditContext,
    owner_user_id: Uuid,
    old_key_id: Uuid,
    new_key_id: Uuid,
    kind: ApiKeyAuditKind,
) -> NewAuditEvent {
    event(
        ctx,
        owner_user_id,
        kind.rotate_op_type(),
        "api_key",
        Some(old_key_id),
        json!({ "created_key_id": new_key_id }),
    )
}

pub(crate) fn api_key_revoked(
    ctx: AuditContext,
    owner_user_id: Uuid,
    key_id: Uuid,
    reason: Option<&str>,
    kind: ApiKeyAuditKind,
) -> NewAuditEvent {
    let metadata = match reason {
        Some(reason) => json!({ "reason": reason }),
        None => json!({}),
    };
    event(
        ctx,
        owner_user_id,
        kind.revoke_op_type(),
        "api_key",
        Some(key_id),
        metadata,
    )
}

pub(crate) fn account_deleted(
    ctx: AuditContext,
    account_id: Uuid,
    counts: AccountDeleteCounts,
) -> NewAuditEvent {
    event(
        ctx,
        account_id,
        "account.delete",
        "account",
        Some(account_id),
        json!({
            "deactivated_agents": counts.deactivated_agents,
            "revoked_api_keys": counts.revoked_api_keys,
            "revoked_browser_sessions": counts.revoked_browser_sessions,
            "disconnected_connections": counts.disconnected_connections,
        }),
    )
}

impl ApiKeyAuditKind {
    fn create_op_type(self) -> &'static str {
        match self {
            ApiKeyAuditKind::User => "user_key.create",
            ApiKeyAuditKind::Agent => "agent_key.create",
        }
    }

    fn rotate_op_type(self) -> &'static str {
        match self {
            ApiKeyAuditKind::User => "user_key.rotate",
            ApiKeyAuditKind::Agent => "agent_key.rotate",
        }
    }

    fn revoke_op_type(self) -> &'static str {
        match self {
            ApiKeyAuditKind::User => "user_key.revoke",
            ApiKeyAuditKind::Agent => "agent_key.revoke",
        }
    }
}

fn event(
    ctx: AuditContext,
    owner_user_id: Uuid,
    op_type: &'static str,
    resource_type: &'static str,
    resource_id: Option<Uuid>,
    metadata: serde_json::Value,
) -> NewAuditEvent {
    NewAuditEvent {
        owner_user_id: Some(owner_user_id),
        actor_account_id: Some(ctx.actor_account_id),
        source: ctx.source.as_str(),
        op_type,
        resource_type,
        resource_id,
        metadata,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_updated_records_changed_fields() {
        let actor = Uuid::from_u128(1);
        let owner = Uuid::from_u128(2);
        let space = Uuid::from_u128(3);

        let event = space_updated(
            AuditContext::rest(actor),
            owner,
            space,
            vec!["name", "sort_order"],
        );

        assert_eq!(event.owner_user_id, Some(owner));
        assert_eq!(event.actor_account_id, Some(actor));
        assert_eq!(event.source, "rest");
        assert_eq!(event.op_type, "space.update");
        assert_eq!(event.resource_type, "space");
        assert_eq!(event.resource_id, Some(space));
        assert_eq!(
            event.metadata.get("changed_fields"),
            Some(&json!(["name", "sort_order"]))
        );
    }

    #[test]
    fn api_key_rotated_is_indexed_by_old_key() {
        let actor = Uuid::from_u128(1);
        let owner = Uuid::from_u128(2);
        let old_key = Uuid::from_u128(3);
        let new_key = Uuid::from_u128(4);

        let event = api_key_rotated(
            AuditContext::rest(actor),
            owner,
            old_key,
            new_key,
            ApiKeyAuditKind::Agent,
        );

        assert_eq!(event.owner_user_id, Some(owner));
        assert_eq!(event.actor_account_id, Some(actor));
        assert_eq!(event.source, "rest");
        assert_eq!(event.op_type, "agent_key.rotate");
        assert_eq!(event.resource_type, "api_key");
        assert_eq!(event.resource_id, Some(old_key));
        assert_eq!(event.metadata.get("created_key_id"), Some(&json!(new_key)));
    }

    #[test]
    fn account_deleted_records_summary_counts() {
        let actor = Uuid::from_u128(1);
        let account = Uuid::from_u128(2);

        let event = account_deleted(
            AuditContext::rest(actor),
            account,
            AccountDeleteCounts {
                deactivated_agents: 2,
                revoked_api_keys: 3,
                revoked_browser_sessions: 4,
                disconnected_connections: 5,
            },
        );

        assert_eq!(event.owner_user_id, Some(account));
        assert_eq!(event.actor_account_id, Some(actor));
        assert_eq!(event.source, "rest");
        assert_eq!(event.op_type, "account.delete");
        assert_eq!(event.resource_type, "account");
        assert_eq!(event.resource_id, Some(account));
        assert_eq!(event.metadata.get("deactivated_agents"), Some(&json!(2)));
        assert_eq!(event.metadata.get("revoked_api_keys"), Some(&json!(3)));
        assert_eq!(
            event.metadata.get("revoked_browser_sessions"),
            Some(&json!(4))
        );
        assert_eq!(
            event.metadata.get("disconnected_connections"),
            Some(&json!(5))
        );
    }
}
