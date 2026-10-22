//! Shared invocation-history capture for external command surfaces.

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use notegate_command::{CommandTool, PurposeValidationError, validate_purpose};
use notegate_db::{CommandInvocationRepo, NewCommandInvocation};
use notegate_model::{Caller, CallerIdentity};
use notegate_service::files::parse_target;
use serde_json::Value;

pub(crate) mod redaction;
#[cfg(test)]
pub(crate) mod test_support;
use crate::observability::CommandInvocationMetrics;

pub(crate) type InvocationWriteFuture<'a> =
    Pin<Box<dyn Future<Output = notegate_core::Result<()>> + Send + 'a>>;

/// Best-effort invocation history only; transactional audit events stay in their repositories.
pub(crate) trait InvocationWriter: Send + Sync {
    fn write<'a>(&'a self, invocation: NewCommandInvocation<'a>) -> InvocationWriteFuture<'a>;
}

impl InvocationWriter for CommandInvocationRepo {
    fn write<'a>(&'a self, invocation: NewCommandInvocation<'a>) -> InvocationWriteFuture<'a> {
        Box::pin(self.insert(invocation))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct InvocationCapture<'a> {
    pub(crate) writer: &'a dyn InvocationWriter,
    pub(crate) metrics_enabled: bool,
}

/// Shared completion boundary; adapters still interpret and redact their own results.
pub(crate) struct InvocationCompletion {
    metrics: CommandInvocationMetrics,
    started: Instant,
    history_started: Instant,
    outcome: &'static str,
}

impl InvocationCompletion {
    pub(crate) fn new(
        metrics: CommandInvocationMetrics,
        started: Instant,
        elapsed: Duration,
        error_code: Option<&str>,
    ) -> Self {
        let outcome = invocation_outcome(error_code);
        metrics.execution_finished(outcome, elapsed);
        Self {
            metrics,
            started,
            history_started: Instant::now(),
            outcome,
        }
    }

    pub(crate) async fn record(
        self,
        writer: &dyn InvocationWriter,
        caller: &Caller,
        invocation: InvocationRecord<'_>,
    ) {
        let recorded = record(writer, caller, invocation).await;
        self.metrics
            .history_finished(recorded, self.history_started.elapsed());
        self.finish();
    }

    pub(crate) fn finish(self) {
        self.metrics.finish(self.outcome, self.started.elapsed());
    }
}

fn invocation_outcome(error_code: Option<&str>) -> &'static str {
    if error_code.is_some() {
        "error"
    } else {
        "success"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvocationSurface {
    Mcp,
    Cli,
    Api,
}

impl InvocationSurface {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Cli => "cli",
            Self::Api => "api",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct InvocationId(pub(crate) uuid::Uuid);

pub(crate) struct InvocationRecord<'a> {
    pub(crate) id: Option<uuid::Uuid>,
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

pub(crate) struct InvocationMetadata<'a> {
    pub(crate) op: Option<&'a str>,
    purpose: Result<Option<&'a str>, PurposeValidationError>,
    pub(crate) space_name: Option<String>,
}

impl<'a> InvocationMetadata<'a> {
    pub(crate) fn from_input(tool: &str, input: &'a Value) -> Self {
        let op = canonical_op(tool, input.get("op").and_then(Value::as_str));
        let known_tool = CommandTool::parse(tool);
        let purpose = if known_tool == Some(CommandTool::Me) {
            Ok(None)
        } else {
            input
                .get("purpose")
                .and_then(Value::as_str)
                .map(|purpose| validate_purpose(purpose).map(|()| purpose))
                .transpose()
        };
        let space_name = if known_tool == Some(CommandTool::Read) && op == Some("changes") {
            invocation_space_name(input.get("target").and_then(Value::as_str))
        } else {
            None
        };
        Self {
            op,
            purpose,
            space_name,
        }
    }

    pub(crate) fn purpose(&self) -> Option<&'a str> {
        self.purpose.as_ref().ok().copied().flatten()
    }

    pub(crate) fn validate_purpose(&self) -> Result<(), &PurposeValidationError> {
        self.purpose.as_ref().map(|_| ())
    }
}

/// Return whether history was stored, without changing the command result.
pub(crate) async fn record(
    writer: &dyn InvocationWriter,
    caller: &Caller,
    invocation: InvocationRecord<'_>,
) -> bool {
    let (owner_user_id, caller_kind) = match &caller.identity {
        CallerIdentity::User(_) => (caller.account_id(), "user"),
        CallerIdentity::Agent(agent) => (agent.owner_user_id, "agent"),
    };
    let outcome = invocation_outcome(invocation.error_code);
    let duration_ms = i64::try_from(invocation.elapsed_ms).unwrap_or(i64::MAX);

    if let Err(error) = writer
        .write(NewCommandInvocation {
            invocation_id: invocation.id,
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
        return false;
    }
    true
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn me_metadata_never_extracts_a_purpose_from_rejected_input() {
        for purpose in ["must not be persisted", " "] {
            let input = json!({"purpose": purpose, "op": "changes", "target": "Daily Research:/"});
            let metadata = InvocationMetadata::from_input("me", &input);

            assert_eq!(metadata.purpose(), None);
            assert_eq!(metadata.validate_purpose(), Ok(()));
            assert_eq!(metadata.op, None);
            assert_eq!(metadata.space_name, None);
        }
    }

    #[test]
    fn invalid_purpose_is_rejected_and_omitted_from_history_metadata() {
        for purpose in [
            " ".to_owned(),
            "가".repeat(notegate_command::PURPOSE_MAX_CHARS + 1),
        ] {
            let input = json!({"purpose": purpose, "op": "spaces"});
            let metadata = InvocationMetadata::from_input("read", &input);

            assert!(metadata.validate_purpose().is_err());
            assert_eq!(metadata.purpose(), None);
            assert_eq!(metadata.op, Some("spaces"));
        }
    }

    #[test]
    fn only_changes_metadata_keeps_a_valid_space_name() {
        let input = json!({"purpose": "inspect changes", "op": "changes", "target": "Daily Research:/private/note.md"});
        let metadata = InvocationMetadata::from_input("read", &input);
        assert_eq!(metadata.op, Some("changes"));
        assert_eq!(metadata.purpose(), Some("inspect changes"));
        assert_eq!(metadata.space_name.as_deref(), Some("Daily Research"));

        for (tool, op, target) in [
            ("read", "read", "Daily Research:/private/note.md"),
            ("search", "changes", "Daily Research:/"),
            ("read", "changes", "missing-colon"),
        ] {
            let input = json!({"op": op, "target": target});
            let metadata = InvocationMetadata::from_input(tool, &input);
            assert_eq!(metadata.space_name, None);
            assert_eq!(metadata.purpose(), None);
            assert_eq!(metadata.validate_purpose(), Ok(()));
        }
    }
}
