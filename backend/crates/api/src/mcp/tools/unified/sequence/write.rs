use super::super::{ManageOperationSchema, WriteEditEntrySchema, WriteOperationSchema};
use super::*;

use std::future::Future;

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "tool", rename_all = "snake_case")]
#[schemars(inline)]
enum WriteSequenceCommandSchema {
    Write(SequenceWriteCommandSchema),
    Manage(SequenceManageCommandSchema),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceWriteCommandSchema {
    /// Write operation: write/append/patch/edit.
    op: WriteOperationSchema,
    /// Text target in `<space>:/absolute/path` form.
    target: String,
    /// Text content for write/append.
    content: Option<String>,
    /// Patch or line-edit entries for patch/edit.
    #[schemars(with = "Option<Vec<WriteEditEntrySchema>>")]
    edits: Option<Vec<Value>>,
    /// Create missing text for write/append.
    #[serde(default)]
    create: bool,
    /// Insert a newline before appended content when needed.
    #[serde(default)]
    ensure_newline: bool,
    /// Optimistic write guard.
    expected_sha256: Option<String>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceManageCommandSchema {
    /// Manage operation: mkdir/mv/cp/rm.
    op: ManageOperationSchema,
    /// Target for mkdir/rm.
    target: Option<String>,
    /// Source target for mv/cp.
    source: Option<String>,
    /// Destination target for mv/cp.
    destination: Option<String>,
    /// Create missing parent folders for mkdir.
    #[serde(default)]
    parents: bool,
    /// Required for folder cp/rm.
    #[serde(default)]
    recursive: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunWriteSequenceInput {
    /// Reason for this MCP invocation. Required once at the top level; commands inherit it and must not include purpose; maximum 200 characters.
    pub purpose: String,
    /// Write/manage command objects. Each includes tool and op, omits purpose and args; 1..20.
    #[schemars(with = "Vec<WriteSequenceCommandSchema>", length(min = 1, max = 20))]
    pub commands: Vec<Value>,
}

pub async fn run_write_sequence(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<RunWriteSequenceInput>,
) -> Result<Json<Value>, ErrorData> {
    validate_sequence_command_count(input.commands.len(), SequenceKind::Write)?;
    let command_count = input.commands.len();
    let commands = prepare_sequence_commands(input.commands, &input.purpose, SequenceKind::Write)?;
    let context = adapter::context(parts)?;
    let (outcomes, skipped) = collect_write_outcomes(commands, command_count, |command| {
        execute_sequence_command(state, &context, command, &input.purpose)
    })
    .await;
    Ok(Json(sequence_response(outcomes, skipped)))
}

pub(super) async fn collect_write_outcomes<F, Fut>(
    commands: Vec<PreparedSequenceCommand>,
    command_count: usize,
    mut execute: F,
) -> (Vec<SequenceOutcome>, usize)
where
    F: FnMut(PreparedSequenceCommand) -> Fut,
    Fut: Future<Output = SequenceOutcome>,
{
    let mut outcomes = Vec::with_capacity(command_count);
    for command in commands {
        let outcome = execute(command).await;
        let failed = outcome.result.is_err();
        outcomes.push(outcome);
        if failed {
            break;
        }
    }

    let skipped = command_count.saturating_sub(outcomes.len());
    (outcomes, skipped)
}
