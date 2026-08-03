//! Shared structured contracts for MCP recovery instructions and errors.

use rmcp::ErrorData;
use serde::Serialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolCallSpec {
    pub tool: String,
    pub input: Value,
}

impl ToolCallSpec {
    pub fn new(tool: impl Into<String>, input: Value) -> Self {
        Self {
            tool: tool.into(),
            input,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolCallStep {
    #[serde(flatten)]
    pub call: ToolCallSpec,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequiredField {
    pub field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A machine-readable instruction that can be returned by either a successful
/// tool response or a recoverable error.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpAction {
    AddFields {
        fields: Vec<RequiredField>,
    },
    RemoveFields {
        fields: Vec<String>,
    },
    ReplaceField {
        field: String,
        value: Value,
    },
    ChooseValue {
        field: String,
        choices: Vec<Value>,
    },
    CallTool {
        #[serde(flatten)]
        call: ToolCallSpec,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        instruction: Option<String>,
    },
    RebuildSnapshot {
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        baseline_call: Option<ToolCallSpec>,
    },
    StoreCursor {
        reason: String,
        cursor: String,
    },
    HttpUpload {
        transfer_field: String,
        instruction: String,
        then: ToolCallSpec,
    },
    HttpUploadParts {
        transfers_field: String,
        collect_response_header: String,
        max_concurrency: usize,
        instruction: String,
        repeat: ToolCallStep,
        then: ToolCallStep,
    },
    HttpDownload {
        transfer_field: String,
        instruction: String,
    },
    Done,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct McpErrorData {
    pub kind: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recoverable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<McpAction>,
    #[serde(flatten)]
    pub details: Map<String, Value>,
}

impl McpErrorData {
    pub fn basic(kind: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            code: code.into(),
            retryable: None,
            recoverable: None,
            hint: None,
            next_action: None,
            details: Map::new(),
        }
    }

    pub fn actionable_input(
        code: impl Into<String>,
        hint: impl Into<String>,
        next_action: McpAction,
    ) -> Self {
        Self {
            kind: "invalid_input".to_owned(),
            code: code.into(),
            retryable: Some(false),
            recoverable: Some(true),
            hint: Some(hint.into()),
            next_action: Some(next_action),
            details: Map::new(),
        }
    }

    pub fn into_value(self) -> Value {
        json!(self)
    }
}

pub fn error_json(error: ErrorData) -> Value {
    json!({
        "code": error.code.0,
        "message": error.message,
        "data": error.data,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn actions_keep_the_public_tagged_shape() {
        let action = McpAction::CallTool {
            call: ToolCallSpec::new("read", json!({"op": "changes", "target": "daily:/"})),
            reason: Some("continue".to_owned()),
            instruction: None,
        };

        assert_eq!(
            json!(action),
            json!({
                "kind": "call_tool",
                "tool": "read",
                "input": {"op": "changes", "target": "daily:/"},
                "reason": "continue",
            })
        );
    }

    #[test]
    fn actionable_error_uses_the_shared_action_contract() {
        let data = McpErrorData::actionable_input(
            "required_fields_missing",
            "Add every required field and retry.",
            McpAction::AddFields {
                fields: vec![RequiredField {
                    field: "purpose".to_owned(),
                    description: Some("Why this tool call is needed.".to_owned()),
                }],
            },
        )
        .into_value();

        assert_eq!(data["kind"], "invalid_input");
        assert_eq!(data["recoverable"], true);
        assert_eq!(data["next_action"]["kind"], "add_fields");
        assert_eq!(data["next_action"]["fields"][0]["field"], "purpose");
    }
}
