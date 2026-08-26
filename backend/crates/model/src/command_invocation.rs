//! External command invocation history read model for user self-review.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandInvocationSurface {
    Mcp,
    Cli,
}

impl CommandInvocationSurface {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Cli => "cli",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandInvocation {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub actor_account_id: Uuid,
    pub caller_kind: String,
    pub surface: String,
    pub tool: String,
    pub op: Option<String>,
    pub purpose: Option<String>,
    pub space_name: Option<String>,
    pub input: Value,
    pub response: Option<Value>,
    pub outcome: String,
    pub error_code: Option<String>,
    pub duration_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ListCommandInvocations {
    pub surface: CommandInvocationSurface,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandInvocationCursor {
    pub created_at: DateTime<Utc>,
    pub id: i64,
    pub surface: CommandInvocationSurface,
}

#[derive(Debug, Clone)]
pub struct CommandInvocationPage {
    pub items: Vec<CommandInvocation>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_contract_accepts_only_mcp_and_cli() -> Result<(), serde_json::Error> {
        assert_eq!(
            serde_json::from_str::<CommandInvocationSurface>("\"mcp\"")?,
            CommandInvocationSurface::Mcp
        );
        assert_eq!(
            serde_json::from_str::<CommandInvocationSurface>("\"cli\"")?,
            CommandInvocationSurface::Cli
        );
        assert!(serde_json::from_str::<CommandInvocationSurface>("\"command_api\"").is_err());
        Ok(())
    }
}
