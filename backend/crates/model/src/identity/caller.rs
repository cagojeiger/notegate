use serde::{Deserialize, Serialize};

use crate::account::Account;
use crate::agent::Agent;
use crate::user::User;

/// The channel an authenticated request arrived on.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Browser,
    Api,
    Mcp,
}

/// The kind-specific detail of an authenticated caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallerIdentity {
    User(User),
    Agent(Agent),
}

/// An authenticated caller: the common account plus its kind-specific detail
/// and the channel it arrived on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Caller {
    pub account: Account,
    pub identity: CallerIdentity,
    pub channel: Channel,
}

impl Caller {
    /// The caller's account id (equals the user or agent id).
    pub fn account_id(&self) -> uuid::Uuid {
        self.account.id
    }

    /// The user OAuth detail, if this caller is a user.
    pub fn user(&self) -> Option<&User> {
        match &self.identity {
            CallerIdentity::User(user) => Some(user),
            CallerIdentity::Agent(_) => None,
        }
    }

    /// The agent detail, if this caller is an agent.
    pub fn agent(&self) -> Option<&Agent> {
        match &self.identity {
            CallerIdentity::Agent(agent) => Some(agent),
            CallerIdentity::User(_) => None,
        }
    }
}
