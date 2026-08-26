//! Shared invocation-history capture for external command surfaces.

use notegate_command::CommandTool;
use notegate_db::NewCommandInvocation;
use notegate_model::{Caller, CallerIdentity};
use notegate_service::files::parse_target;
use serde_json::Value;

pub(crate) mod redaction;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvocationSurface {
    Mcp,
    Cli,
}

impl InvocationSurface {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Cli => "cli",
        }
    }
}

pub(crate) struct InvocationRecord<'a> {
    pub(crate) surface: InvocationSurface,
    pub(crate) tool: &'a str,
    pub(crate) op: Option<&'a str>,
    pub(crate) purpose: Option<&'a str>,
    pub(crate) space_name: Option<&'a str>,
    pub(crate) input: &'a Value,
    pub(crate) response: Option<&'a Value>,
    pub(crate) error_code: Option<&'a str>,
    pub(crate) elapsed_ms: u128,
}

pub(crate) async fn record(state: &AppState, caller: &Caller, invocation: InvocationRecord<'_>) {
    let (owner_user_id, caller_kind) = match &caller.identity {
        CallerIdentity::User(_) => (caller.account_id(), "user"),
        CallerIdentity::Agent(agent) => (agent.owner_user_id, "agent"),
    };
    let outcome = if invocation.error_code.is_some() {
        "error"
    } else {
        "success"
    };
    let duration_ms = i64::try_from(invocation.elapsed_ms).unwrap_or(i64::MAX);

    if let Err(error) = state
        .command_invocations
        .insert(NewCommandInvocation {
            owner_user_id,
            actor_account_id: caller.account_id(),
            caller_kind,
            surface: invocation.surface.as_str(),
            tool: invocation.tool,
            op: invocation.op,
            purpose: invocation.purpose,
            space_name: invocation.space_name,
            input: invocation.input,
            response: invocation.response,
            outcome,
            error_code: invocation.error_code,
            duration_ms,
        })
        .await
    {
        tracing::warn!(
            surface = invocation.surface.as_str(),
            tool = invocation.tool,
            op = invocation.op,
            outcome,
            error = %error,
            "failed to record command invocation history"
        );
    }
}

pub(crate) fn canonical_tool(tool: &str) -> &'static str {
    CommandTool::parse(tool).map_or("unknown", CommandTool::as_str)
}

pub(crate) fn canonical_op<'a>(tool: &str, op: Option<&'a str>) -> Option<&'a str> {
    match (CommandTool::parse(tool), op?) {
        (
            Some(CommandTool::Read),
            op @ ("spaces" | "ls" | "tree" | "stat" | "read" | "changes"),
        )
        | (Some(CommandTool::Search), op @ ("find" | "grep"))
        | (Some(CommandTool::Write), op @ ("write" | "append" | "patch" | "edit"))
        | (Some(CommandTool::Manage), op @ ("mkdir" | "mv" | "cp" | "rm"))
        | (
            Some(CommandTool::FileUpload),
            op @ ("begin_upload" | "prepare_parts" | "complete_upload" | "abort_upload"),
        ) => Some(op),
        _ => None,
    }
}

/// Extract the validated Space-name segment used by invocation list summaries.
pub(crate) fn invocation_space_name(target: Option<&str>) -> Option<String> {
    target
        .and_then(|target| parse_target(target).ok())
        .map(|target| target.space)
}

pub(crate) fn sequence_error_code(result: &Value) -> Option<String> {
    if result.get("ok").and_then(Value::as_bool) != Some(false) {
        return None;
    }

    result
        .get("results")
        .and_then(Value::as_array)
        .and_then(|results| {
            results
                .iter()
                .find(|item| item.get("ok").and_then(Value::as_bool) == Some(false))
        })
        .and_then(|item| {
            item.pointer("/error/data/code")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    item.pointer("/error/code")
                        .and_then(Value::as_i64)
                        .map(|code| code.to_string())
                })
        })
}
