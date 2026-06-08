//! The common actor identity. Every authenticated caller — user or agent —
//! resolves to exactly one `Account`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What kind of actor an account represents.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    /// A human user authenticated via OAuth.
    User,
    /// A machine agent authenticated via an agent key.
    Agent,
}

impl AccountKind {
    /// Parse the storage representation, returning `None` for unknown values.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }

    /// The storage representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }
}

/// The common actor identity row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: Uuid,
    pub kind: AccountKind,
    pub display_name: String,
    pub is_active: bool,
    pub deleted_at: Option<DateTime<Utc>>,
    pub deleted_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A lightweight reference to an account, used in API output where only the
/// identity and kind matter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountRef {
    pub id: Uuid,
    pub kind: AccountKind,
    pub display_name: String,
}

impl From<&Account> for AccountRef {
    fn from(account: &Account) -> Self {
        Self {
            id: account.id,
            kind: account.kind,
            display_name: account.display_name.clone(),
        }
    }
}
