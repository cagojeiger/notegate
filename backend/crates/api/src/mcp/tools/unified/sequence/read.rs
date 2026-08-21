use super::*;

use std::future::Future;

use futures_util::{StreamExt, stream};

pub(super) const READ_SEQUENCE_CONCURRENCY: usize = 4;

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "tool", rename_all = "snake_case")]
#[schemars(inline)]
enum ReadSequenceCommandSchema {
    Read(SequenceReadCommandSchema),
    Search(SequenceSearchCommandSchema),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
enum SequenceReadOperationSchema {
    Spaces,
    Ls,
    Tree,
    Stat,
    Read,
    Changes,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case", inline)]
enum SequenceSearchOperationSchema {
    Find,
    Grep,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceReadCommandSchema {
    /// Read operation: spaces/ls/tree/stat/read/changes.
    op: SequenceReadOperationSchema,
    /// Target in `<space>:/absolute/path` form when required by the operation.
    target: Option<String>,
    /// Optional exact, case-sensitive space name filter for spaces.
    name: Option<String>,
    /// Tree depth for tree.
    depth: Option<i64>,
    /// Page size.
    limit: Option<i64>,
    /// Opaque pagination cursor.
    cursor: Option<String>,
    /// Changes direction: older/newer.
    direction: Option<String>,
    /// 1-based first line for read.
    start_line: Option<i64>,
    /// Maximum lines for read.
    max_lines: Option<i64>,
    /// Maximum bytes for read.
    max_bytes: Option<usize>,
    /// Conditional read guard.
    if_none_match_sha256: Option<String>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceSearchCommandSchema {
    /// Search operation: find/grep.
    op: SequenceSearchOperationSchema,
    /// Scope target in `<space>:/absolute/path` form.
    target: String,
    /// Search query.
    q: String,
    /// Find node kind filter: folder/text/file.
    kind: Option<String>,
    /// Find or grep match mode.
    #[serde(rename = "match")]
    match_mode: Option<String>,
    /// Grep line detail: none/first/all.
    lines: Option<String>,
    /// Optional path glob includes.
    include: Option<Vec<String>>,
    /// Optional path glob excludes.
    exclude: Option<Vec<String>>,
    /// Page size.
    limit: Option<i64>,
    /// Opaque pagination cursor.
    cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunReadSequenceInput {
    /// Reason for this MCP invocation. Required once at the top level; commands inherit it and must not include purpose; maximum 200 characters.
    pub purpose: String,
    /// Read/search command objects. Each includes tool and op, omits purpose and args; 1..20.
    #[schemars(with = "Vec<ReadSequenceCommandSchema>", length(min = 1, max = 20))]
    pub commands: Vec<Value>,
}

pub async fn run_read_sequence(
    state: &AppState,
    parts: &Parts,
    Parameters(input): Parameters<RunReadSequenceInput>,
) -> Result<Json<Value>, ErrorData> {
    validate_sequence_command_count(input.commands.len(), SequenceKind::Read)?;
    let commands = prepare_sequence_commands(input.commands, &input.purpose, SequenceKind::Read)?;
    let purpose = input.purpose;
    let outcomes = collect_read_outcomes(commands, |command| {
        execute_sequence_command(state, parts, command, &purpose)
    })
    .await;
    Ok(Json(sequence_response(outcomes, 0)))
}

pub(super) async fn collect_read_outcomes<F, Fut>(
    commands: Vec<PreparedSequenceCommand>,
    execute: F,
) -> Vec<SequenceOutcome>
where
    F: FnMut(PreparedSequenceCommand) -> Fut,
    Fut: Future<Output = SequenceOutcome>,
{
    let mut outcomes = stream::iter(commands)
        .map(execute)
        .buffer_unordered(READ_SEQUENCE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    outcomes.sort_by_key(|outcome| outcome.index);
    outcomes
}
