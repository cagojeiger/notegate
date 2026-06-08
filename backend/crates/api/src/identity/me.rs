//! The shared `me` identity builder used by both REST `GET /api/v1/me` and the
//! MCP `me` tool, so the two surfaces stay aligned (`docs/spec/mcp/identity.md`).
//!
//! The shape is `{ account, user?, agent?, capabilities }`. Workspace-specific
//! roles are intentionally excluded; callers enumerate them through the
//! Workspaces category (`GET /api/v1/workspaces` / `workspaces_list`).

use notegate_model::account::AccountKind;
use notegate_model::{Caller, CallerIdentity};
use schemars::JsonSchema;
use serde::Serialize;
use utoipa::ToSchema;

/// A lightweight account reference, mirroring `docs/spec/rest/README.md`'s Account ref.
#[derive(Debug, Clone, Serialize, JsonSchema, ToSchema, PartialEq, Eq)]
pub struct AccountRefOutput {
    pub id: String,
    pub kind: String,
    pub display_name: String,
}

/// User OAuth detail, present for user callers.
#[derive(Debug, Clone, Serialize, JsonSchema, ToSchema, PartialEq, Eq)]
pub struct UserDetailOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Agent detail, present for agent callers.
#[derive(Debug, Clone, Serialize, JsonSchema, ToSchema, PartialEq, Eq)]
pub struct AgentDetailOutput {
    pub name: String,
}

/// Global, non-workspace capabilities for the authenticated caller.
#[derive(Debug, Clone, Serialize, JsonSchema, ToSchema, PartialEq, Eq)]
pub struct CapabilitiesOutput {
    /// The caller may create workspaces as the workspace owner.
    pub can_create_workspace: bool,
    /// The caller may create/delete agents and mint/revoke agent keys.
    pub can_manage_agents: bool,
}

/// The current caller, optional user/agent detail, and global capabilities.
#[derive(Debug, Clone, Serialize, JsonSchema, ToSchema, PartialEq, Eq)]
pub struct MeOutput {
    pub account: AccountRefOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<UserDetailOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentDetailOutput>,
    pub capabilities: CapabilitiesOutput,
}

pub fn build_me(caller: &Caller) -> MeOutput {
    let account = AccountRefOutput {
        id: caller.account.id.to_string(),
        kind: caller.account.kind.as_str().to_owned(),
        display_name: caller.account.display_name.clone(),
    };
    let (user, agent) = match &caller.identity {
        CallerIdentity::User(user) => (
            Some(UserDetailOutput {
                sub: user.sub.clone(),
                email: user.email.clone(),
            }),
            None,
        ),
        CallerIdentity::Agent(agent) => (
            None,
            Some(AgentDetailOutput {
                name: agent.name.clone(),
            }),
        ),
    };
    let capabilities = CapabilitiesOutput {
        can_create_workspace: caller.account.kind == AccountKind::User,
        can_manage_agents: caller.account.kind == AccountKind::User,
    };
    MeOutput {
        account,
        user,
        agent,
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use chrono::Utc;
    use notegate_model::account::{Account, AccountKind};
    use notegate_model::agent::Agent;
    use notegate_model::{Caller, CallerIdentity, Channel, User};
    use uuid::Uuid;

    use super::build_me;

    #[test]
    fn build_me_uses_nested_user_identity_shape() {
        let now = Utc::now();
        let account = Account {
            id: Uuid::nil(),
            kind: AccountKind::User,
            display_name: "Test User".to_owned(),
            is_active: true,
            deleted_at: None,
            deleted_by: None,
            created_at: now,
            updated_at: now,
        };
        let user = User {
            id: Uuid::nil(),
            sub: Some("sub-1".to_owned()),
            email: Some("user@example.test".to_owned()),
            anonymized_at: None,
        };
        let caller = Caller {
            account,
            identity: CallerIdentity::User(user),
            channel: Channel::Api,
        };
        let out = build_me(&caller);
        assert_eq!(out.account.id, "00000000-0000-0000-0000-000000000000");
        assert_eq!(out.account.kind, "user");
        assert_eq!(out.account.display_name, "Test User");
        let user = out.user.expect("user detail present");
        assert_eq!(user.sub.as_deref(), Some("sub-1"));
        assert_eq!(user.email.as_deref(), Some("user@example.test"));
        assert!(out.agent.is_none());
        assert!(out.capabilities.can_create_workspace);
        assert!(out.capabilities.can_manage_agents);
    }

    #[test]
    fn build_me_uses_agent_detail_for_agent_caller() {
        let now = Utc::now();
        let account = Account {
            id: Uuid::nil(),
            kind: AccountKind::Agent,
            display_name: "research-agent".to_owned(),
            is_active: true,
            deleted_at: None,
            deleted_by: None,
            created_at: now,
            updated_at: now,
        };
        let agent = Agent {
            id: Uuid::nil(),
            name: "research-agent".to_owned(),
            created_by: Uuid::nil(),
        };
        let caller = Caller {
            account,
            identity: CallerIdentity::Agent(agent),
            channel: Channel::Mcp,
        };
        let out = build_me(&caller);
        assert_eq!(out.account.kind, "agent");
        assert!(out.user.is_none());
        assert_eq!(
            out.agent.expect("agent detail present").name,
            "research-agent"
        );
        assert!(!out.capabilities.can_create_workspace);
        assert!(!out.capabilities.can_manage_agents);
    }
}
