use std::future::Future;

use super::{PreparedSequenceCommand, SequenceOutcome};

pub(super) async fn collect<F, Fut>(
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
