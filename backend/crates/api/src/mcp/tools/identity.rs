use axum::http::request::Parts;
use notegate_model::Caller;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Serialize;

use super::resolve::invalid_input_error;
use crate::identity::me::{MeOutput, build_me};

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct McpMeOutput {
    #[serde(flatten)]
    pub identity: MeOutput,
    /// Version of the running NoteGate server binary.
    pub server_version: String,
}

impl McpMeOutput {
    fn new(identity: MeOutput) -> Self {
        Self {
            identity,
            server_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

pub fn call(parts: &Parts) -> Result<Json<McpMeOutput>, ErrorData> {
    let caller = parts
        .extensions
        .get::<Caller>()
        .ok_or_else(|| invalid_input_error("authenticated caller extension missing"))?;
    Ok(Json(McpMeOutput::new(build_me(caller))))
}

#[cfg(test)]
mod tests {
    use crate::identity::me::{AccountRefOutput, CapabilitiesOutput};

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
