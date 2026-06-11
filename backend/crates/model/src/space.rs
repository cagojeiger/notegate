//! Spaces and agent connections.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A user-owned AI-native file space.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Space {
    pub id: Uuid,
    pub name: String,
    pub owner_user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub deleted_by_user_id: Option<Uuid>,
    pub purge_after: Option<DateTime<Utc>>,
}

/// Agent permission inside one space.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Read,
    Write,
}

impl Permission {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }

    pub fn allows_write(self) -> bool {
        self == Self::Write
    }
}

/// A user-managed agent connection to one space.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceAgentConnection {
    pub space_id: Uuid,
    pub agent_id: Uuid,
    pub permission: Permission,
    pub connected_by_user_id: Uuid,
    pub connected_at: DateTime<Utc>,
    pub disconnected_at: Option<DateTime<Utc>>,
    pub disconnected_by_user_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct CreateSpace {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct RenameSpace {
    pub space_id: Uuid,
    pub new_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct ListSpaces {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

/// A visible space plus caller permission and derived root node id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceView {
    pub space: Space,
    pub permission: Permission,
    pub root_node_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct SpacePage {
    pub items: Vec<SpaceView>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ListConnections {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectionPage {
    pub items: Vec<SpaceAgentConnection>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectAgent {
    pub space_id: Uuid,
    pub agent_id: Uuid,
    pub permission: Permission,
}
