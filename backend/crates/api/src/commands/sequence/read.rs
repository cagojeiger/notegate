use std::future::Future;

use futures_util::{StreamExt, stream};

use super::{PreparedSequenceCommand, SequenceOutcome};

pub(super) const READ_SEQUENCE_CONCURRENCY: usize = 4;

pub(super) async fn collect<F, Fut>(
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
