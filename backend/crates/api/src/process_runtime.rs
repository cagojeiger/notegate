use std::future::pending;

use futures_util::future::{BoxFuture, FutureExt as _};
use futures_util::stream::{FuturesUnordered, StreamExt as _};
use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::background_jobs::{self, BackgroundJobs};
use crate::state::AppState;
use crate::{metadata_write_behind, observability, reconciliations};

pub(crate) struct ProcessRuntime {
    shutdown: CancellationToken,
    metadata_shutdown: CancellationToken,
    background_jobs: Option<BackgroundJobs>,
    critical_tasks: TaskSet,
    auxiliary_tasks: TaskSet,
}

impl ProcessRuntime {
    pub(crate) fn start(state: &AppState, shutdown: CancellationToken) -> anyhow::Result<Self> {
        let process_mode = state.config.process_mode;
        let background_jobs = if process_mode.runs_worker() {
            Some(background_jobs::spawn(
                state.db.clone(),
                state.config.background_jobs,
                state.config.metrics_enabled,
                shutdown.clone(),
            )?)
        } else {
            None
        };
        let metadata_shutdown = CancellationToken::new();
        let mut critical_tasks = TaskSet::default();
        let mut auxiliary_tasks = TaskSet::default();
        auxiliary_tasks.push(
            "metrics upkeep worker",
            observability::spawn_upkeep(state.metrics.clone(), shutdown.clone()),
        );
        let reconciliation_runtime = if process_mode.runs_worker() {
            let job_kinds = background_jobs
                .as_ref()
                .map(BackgroundJobs::job_kinds)
                .unwrap_or_default();
            Some(reconciliations::spawn(
                &state.db,
                state.object_storage.clone(),
                job_kinds,
                shutdown.clone(),
            )?)
        } else {
            None
        };
        critical_tasks.push("reconciliation runtime", reconciliation_runtime);
        auxiliary_tasks.push(
            "metadata write-behind",
            process_mode.runs_api().then(|| {
                metadata_write_behind::spawn(
                    state.metadata_writes.clone(),
                    state.db.clone(),
                    metadata_shutdown.clone(),
                    state.config.metrics_enabled,
                )
            }),
        );

        Ok(Self {
            shutdown,
            metadata_shutdown,
            background_jobs,
            critical_tasks,
            auxiliary_tasks,
        })
    }

    pub(crate) async fn wait_for_critical_exit(&mut self) -> anyhow::Error {
        loop {
            tokio::select! {
                error = wait_for_background_jobs(&mut self.background_jobs) => return error,
                exit = self.critical_tasks.next_exit() => return task_exit_error(exit),
                exit = self.auxiliary_tasks.next_exit() => log_auxiliary_exit(exit),
            }
        }
    }

    pub(crate) fn begin_shutdown(&self) {
        self.shutdown.cancel();
    }

    pub(crate) async fn join(mut self) {
        self.shutdown.cancel();
        self.metadata_shutdown.cancel();
        self.auxiliary_tasks.join().await;
        self.critical_tasks.join().await;
        if let Some(background_jobs) = self.background_jobs.take() {
            background_jobs.join().await;
        }
    }
}

async fn wait_for_background_jobs(runtime: &mut Option<BackgroundJobs>) -> anyhow::Error {
    match runtime {
        Some(runtime) => runtime.wait_for_critical_exit().await,
        None => pending().await,
    }
}

type TaskExit = (&'static str, Result<(), JoinError>);

#[derive(Default)]
struct TaskSet(FuturesUnordered<BoxFuture<'static, TaskExit>>);

impl TaskSet {
    fn push(&mut self, name: &'static str, task: Option<JoinHandle<()>>) {
        if let Some(task) = task {
            self.0.push(async move { (name, task.await) }.boxed());
        }
    }

    async fn next_exit(&mut self) -> TaskExit {
        match self.0.next().await {
            Some(exit) => exit,
            None => pending().await,
        }
    }

    async fn join(&mut self) {
        while let Some((name, result)) = self.0.next().await {
            if let Err(error) = result {
                tracing::error!(event = "process_runtime.task_join_failed", task = name, %error);
            }
        }
    }
}

fn task_exit_error((name, result): TaskExit) -> anyhow::Error {
    match result {
        Ok(()) => anyhow::anyhow!("{name} stopped unexpectedly"),
        Err(error) => anyhow::anyhow!("{name} task failed: {error}"),
    }
}

fn log_auxiliary_exit((name, result): TaskExit) {
    match result {
        Ok(()) => tracing::error!(
            event = "process_runtime.auxiliary_task_stopped",
            task = name,
        ),
        Err(error) => tracing::error!(
            event = "process_runtime.auxiliary_task_failed",
            task = name,
            %error,
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use super::*;

    fn runtime_with_task(name: &'static str, task: JoinHandle<()>) -> ProcessRuntime {
        let mut critical_tasks = TaskSet::default();
        critical_tasks.push(name, Some(task));
        ProcessRuntime {
            shutdown: CancellationToken::new(),
            metadata_shutdown: CancellationToken::new(),
            background_jobs: None,
            critical_tasks,
            auxiliary_tasks: TaskSet::default(),
        }
    }

    fn empty_runtime() -> ProcessRuntime {
        ProcessRuntime {
            shutdown: CancellationToken::new(),
            metadata_shutdown: CancellationToken::new(),
            background_jobs: None,
            critical_tasks: TaskSet::default(),
            auxiliary_tasks: TaskSet::default(),
        }
    }

    #[tokio::test]
    async fn reports_an_unexpected_reconciliation_runtime_exit() {
        let mut runtime = runtime_with_task("reconciliation runtime", tokio::spawn(async {}));

        let error = runtime.wait_for_critical_exit().await;

        assert_eq!(
            error.to_string(),
            "reconciliation runtime stopped unexpectedly"
        );
    }

    #[tokio::test]
    async fn reports_a_cancelled_reconciliation_runtime_task() {
        let task = tokio::spawn(std::future::pending::<()>());
        task.abort();
        let mut runtime = runtime_with_task("reconciliation runtime", task);

        let error = runtime.wait_for_critical_exit().await;

        assert!(
            error
                .to_string()
                .starts_with("reconciliation runtime task failed:")
        );
    }

    #[tokio::test]
    async fn auxiliary_exit_does_not_stop_the_process() {
        let release_critical = Arc::new(tokio::sync::Notify::new());
        let (auxiliary_stopped, auxiliary_stopped_rx) = tokio::sync::oneshot::channel();
        let mut runtime = runtime_with_task(
            "critical worker",
            tokio::spawn({
                let release_critical = release_critical.clone();
                async move { release_critical.notified().await }
            }),
        );
        runtime.auxiliary_tasks.push(
            "auxiliary worker",
            Some(tokio::spawn(async move {
                let _ = auxiliary_stopped.send(());
            })),
        );
        let mut wait_for_exit = Box::pin(runtime.wait_for_critical_exit());

        assert!(
            auxiliary_stopped_rx.await.is_ok(),
            "auxiliary task should report its exit"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut wait_for_exit)
                .await
                .is_err(),
            "auxiliary task exit must not stop the process"
        );
        release_critical.notify_one();

        let error = wait_for_exit.await;

        assert_eq!(error.to_string(), "critical worker stopped unexpectedly");
    }

    #[tokio::test]
    async fn joins_tasks_remaining_after_a_critical_exit() {
        let shutdown = CancellationToken::new();
        let remaining_stopped = Arc::new(AtomicBool::new(false));
        let mut critical_tasks = TaskSet::default();
        critical_tasks.push("finished worker", Some(tokio::spawn(async {})));
        critical_tasks.push(
            "remaining worker",
            Some(tokio::spawn({
                let shutdown = shutdown.clone();
                let remaining_stopped = remaining_stopped.clone();
                async move {
                    shutdown.cancelled().await;
                    remaining_stopped.store(true, Ordering::SeqCst);
                }
            })),
        );
        let mut runtime = ProcessRuntime {
            shutdown,
            metadata_shutdown: CancellationToken::new(),
            background_jobs: None,
            critical_tasks,
            auxiliary_tasks: TaskSet::default(),
        };

        let error = runtime.wait_for_critical_exit().await;
        runtime.join().await;

        assert_eq!(error.to_string(), "finished worker stopped unexpectedly");
        assert!(remaining_stopped.load(Ordering::SeqCst));
    }

    #[test]
    fn begin_shutdown_cancels_only_the_shared_runtime_token() {
        let runtime = empty_runtime();

        runtime.begin_shutdown();

        assert!(runtime.shutdown.is_cancelled());
        assert!(!runtime.metadata_shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn join_cancels_all_runtime_tokens() {
        let shutdown = CancellationToken::new();
        let metadata_shutdown = CancellationToken::new();
        let runtime = ProcessRuntime {
            shutdown: shutdown.clone(),
            metadata_shutdown: metadata_shutdown.clone(),
            background_jobs: None,
            critical_tasks: TaskSet::default(),
            auxiliary_tasks: TaskSet::default(),
        };

        runtime.join().await;

        assert!(shutdown.is_cancelled());
        assert!(metadata_shutdown.is_cancelled());
    }
}
