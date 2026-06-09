//! Agent accounts and their authentication keys.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An agent account detail. `id` equals the owning `accounts.id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Agent {
    /// Equal to the owning `accounts.id`.
    pub id: Uuid,
    pub name: String,
    /// The account that created this agent.
    pub created_by: Uuid,
}

/// A bearer key that authenticates as a given agent. The plaintext token is
/// never stored — only its hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentKey {
    pub id: Uuid,
    pub agent_id: Uuid,
    /// Hash of the key's plaintext token. Never the plaintext.
    pub token_hash: String,
    pub name: String,
    /// Current contract requires this to be empty.
    pub scopes: Vec<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_by: Option<Uuid>,
}

/// Input to create an agent.
#[derive(Debug, Clone)]
pub struct CreateAgent {
    pub name: String,
}

/// Input to create an agent key.
#[derive(Debug, Clone)]
pub struct CreateAgentKey {
    pub agent_id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Input to list agents created by the caller.
#[derive(Debug, Clone, Default)]
pub struct ListAgents {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// A page of agents.
#[derive(Debug, Clone)]
pub struct AgentPage {
    pub items: Vec<Agent>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}
