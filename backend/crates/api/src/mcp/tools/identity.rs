//! MCP adapter for the shared identity command.

use axum::http::request::Parts;
use rmcp::{ErrorData, Json};

use super::adapter;
use crate::commands;

pub type McpMeOutput = commands::identity::IdentityOutput;

pub fn call(parts: &Parts) -> Result<Json<McpMeOutput>, ErrorData> {
    let context = adapter::context(parts)?;
    Ok(Json(commands::identity::call(&context)))
}

#[cfg(test)]
mod tests {
    use crate::identity::me::{AccountRefOutput, CapabilitiesOutput, MeOutput};

    use super::*;

    #[test]
    fn mcp_me_output_flattens_identity_and_reports_running_version() -> Result<(), serde_json::Error>
    {
        let output = McpMeOutput::new(MeOutput {
            account: AccountRefOutput {
                id: "account-id".to_owned(),
                kind: "agent".to_owned(),
                display_name: "research-agent".to_owned(),
            },
            user: None,
            agent: None,
            capabilities: CapabilitiesOutput {
                can_create_space: false,
                can_manage_agents: false,
            },
        });

        let json = serde_json::to_value(output)?;
        assert_eq!(
            json.pointer("/account/id")
                .and_then(serde_json::Value::as_str),
            Some("account-id")
        );
        assert!(json.get("identity").is_none());
        assert_eq!(
            json.get("server_version")
                .and_then(serde_json::Value::as_str),
            Some(env!("CARGO_PKG_VERSION"))
        );
        Ok(())
    }
}
