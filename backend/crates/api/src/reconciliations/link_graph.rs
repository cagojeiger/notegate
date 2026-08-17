use std::time::Duration;

use notegate_db::{LinkGraphChangeCollection, LinkGraphWorkRepo};
use notegate_reconciliation::{
    Reconciler, ReconciliationContext, ReconciliationDirective, ReconciliationError,
    ReconciliationFailure, ReconciliationFuture, ReconciliationSchedule,
};

const CHANGE_COLLECTION_INTERVAL: Duration = Duration::from_secs(5 * 60);
const CHANGE_COLLECTION_TIMEOUT: Duration = Duration::from_secs(60);
const BACKLOG_CONTINUATION_DELAY: Duration = Duration::from_secs(1);

pub(super) struct LinkGraphChangeCollector {
    work: LinkGraphWorkRepo,
}

impl LinkGraphChangeCollector {
    pub(super) fn new(work: LinkGraphWorkRepo) -> Self {
        Self { work }
    }

    pub(super) fn schedule() -> Result<ReconciliationSchedule, ReconciliationError> {
        ReconciliationSchedule::new(CHANGE_COLLECTION_INTERVAL, CHANGE_COLLECTION_TIMEOUT)
    }
}

impl Reconciler for LinkGraphChangeCollector {
    const KIND: &'static str = "link_graph.change_collector";

    fn reconcile<'a>(&'a self, _context: &'a ReconciliationContext) -> ReconciliationFuture<'a> {
        Box::pin(async move {
            let result = self
                .work
                .collect_changes()
                .await
                .map_err(|error| Box::new(error) as ReconciliationFailure)?;
            match result {
                LinkGraphChangeCollection::Idle => Ok(ReconciliationDirective::Complete),
                LinkGraphChangeCollection::Collected {
                    spaces,
                    events,
                    staged_targets,
                    failed_targets,
                    dispatched_targets,
                    jobs,
                    has_more,
                } => {
                    tracing::debug!(
                        event = "link_graph.changes_collected",
                        spaces,
                        events,
                        staged_targets,
                        failed_targets,
                        dispatched_targets,
                        jobs,
                        has_more,
                    );
                    if has_more {
                        Ok(ReconciliationDirective::ContinueAfter(
                            BACKLOG_CONTINUATION_DELAY,
                        ))
                    } else {
                        Ok(ReconciliationDirective::Complete)
                    }
                }
            }
        })
    }
}
