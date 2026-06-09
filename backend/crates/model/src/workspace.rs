//! Workspaces and the per-account access grants that govern them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A workspace: a named tree whose lifecycle owner is `created_by`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub deleted_by: Option<Uuid>,
    pub purge_after: Option<DateTime<Utc>>,
}

/// A role within a workspace, ordered from least to most privileged.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Read-only: list/stat/read/find/grep.
    Viewer,
    /// Read-write: viewer plus mutations.
    Editor,
    /// Effective full control derived from `workspaces.created_by`; never stored in `workspace_access`.
    Owner,
}

impl Role {
    /// Parse the storage representation, returning `None` for unknown values.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "viewer" => Some(Self::Viewer),
            "editor" => Some(Self::Editor),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }

    /// The storage representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
            Self::Owner => "owner",
        }
    }
}

/// An access grant: one account's role within one workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceAccess {
    pub workspace_id: Uuid,
    pub account_id: Uuid,
    pub role: Role,
    pub granted_by: Option<Uuid>,
    pub granted_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_by: Option<Uuid>,
}

/// Input to create a workspace.
#[derive(Debug, Clone)]
pub struct CreateWorkspace {
    pub name: String,
}

/// Input to rename a workspace.
#[derive(Debug, Clone)]
pub struct RenameWorkspace {
    pub workspace_id: Uuid,
    pub new_name: String,
}

/// Input to list visible workspaces.
#[derive(Debug, Clone, Default)]
pub struct ListWorkspaces {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// Keyset cursor for workspace list order `(created_at, id)`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

/// A workspace plus the caller's role and derived root node id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceView {
    pub workspace: Workspace,
    pub role: Role,
    pub root_node_id: Uuid,
}

/// A page of workspace views.
#[derive(Debug, Clone)]
pub struct WorkspacePage {
    pub items: Vec<WorkspaceView>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// Input to list workspace access grants.
#[derive(Debug, Clone, Default)]
pub struct ListAccess {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// A page of access grants.
#[derive(Debug, Clone)]
pub struct AccessPage {
    pub items: Vec<WorkspaceAccess>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// Input to grant or update an account's role in a workspace.
#[derive(Debug, Clone)]
pub struct GrantAccess {
    pub workspace_id: Uuid,
    pub account_id: Uuid,
    pub role: Role,
}
