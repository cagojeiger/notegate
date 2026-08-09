mod handlers;

use std::sync::Arc;

use handlers::UsageHandler;
use notegate_db::SpaceUsageRepo;
use notegate_jobs::{
    JobHandler, JobQueue, JobQueueResult, QueueReconciler, QueueReconcilerConfig, Worker,
    WorkerConfig,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::state::AppState;

pub(crate) struct BackgroundJobs {
    consumer: Option<JoinHandle<JobQueueResult<()>>>,
    reconciler: Option<JoinHandle<()>>,
    metrics: Option<JoinHandle<()>>,
}

pub(crate) fn spawn(
    state: &AppState,
    shutdown: CancellationToken,
) -> JobQueueResult<BackgroundJobs> {
    let queue = JobQueue::new(state.db.clone());
    let handlers: Vec<Arc<dyn JobHandler>> = vec![Arc::new(UsageHandler::new(
        SpaceUsageRepo::new(state.db.clone()),
    ))];
    let job_kinds = handlers
        .iter()
        .map(|handler| handler.kind().to_owned())
        .collect::<Vec<_>>();
    let worker = Worker::new(
        queue.clone(),
        handlers,
        WorkerConfig {
            concurrency: state.config.background_jobs.concurrency,
            ..WorkerConfig::default()
        },
        worker_id(),
    )?;
    let reconciler = QueueReconciler::new(queue.clone(), QueueReconcilerConfig::default())?;
    let consumer_shutdown = shutdown.clone();
    let reconciler_shutdown = shutdown.clone();
    let consumer = tokio::spawn(async move { worker.run(consumer_shutdown).await });
    let reconciler = tokio::spawn(async move { reconciler.run(reconciler_shutdown).await });
    let metrics = crate::observability::spawn_background_job_metrics(
        state.config.metrics_enabled,
        queue,
        job_kinds,
        shutdown,
    );

    tracing::info!(
        event = "background_jobs.started",
        concurrency = state.config.background_jobs.concurrency
    );
    Ok(BackgroundJobs {
        consumer: Some(consumer),
        reconciler: Some(reconciler),
        metrics,
    })
}

impl BackgroundJobs {
    pub(crate) async fn wait_for_critical_exit(&mut self) -> anyhow::Error {
        enum CriticalExit {
            Consumer(Result<JobQueueResult<()>, tokio::task::JoinError>),
            Reconciler(Result<(), tokio::task::JoinError>),
        }

        let exit = match (self.consumer.as_mut(), self.reconciler.as_mut()) {
            (Some(consumer), Some(reconciler)) => {
                tokio::select! {
                    result = consumer => CriticalExit::Consumer(result),
                    result = reconciler => CriticalExit::Reconciler(result),
                }
            }
            (None, _) => return anyhow::anyhow!("background job consumer is not running"),
            (_, None) => return anyhow::anyhow!("background job reconciler is not running"),
        };

        match exit {
            CriticalExit::Consumer(result) => {
                self.consumer.take();
                match result {
                    Ok(Ok(())) => anyhow::anyhow!("background job consumer stopped unexpectedly"),
                    Ok(Err(error)) => anyhow::anyhow!(error),
                    Err(error) => anyhow::anyhow!("background job consumer task failed: {error}"),
                }
            }
            CriticalExit::Reconciler(result) => {
                self.reconciler.take();
                match result {
                    Ok(()) => anyhow::anyhow!("background job reconciler stopped unexpectedly"),
                    Err(error) => anyhow::anyhow!("background job reconciler task failed: {error}"),
                }
            }
        }
    }

    pub(crate) async fn join(self) {
        if let Some(consumer) = self.consumer {
            match consumer.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::error!(event = "background_jobs.consumer_failed", %error)
                }
                Err(error) => {
                    tracing::error!(event = "background_jobs.consumer_join_failed", %error)
                }
            }
        }
        if let Some(reconciler) = self.reconciler
            && let Err(error) = reconciler.await
        {
            tracing::error!(event = "background_jobs.reconciler_join_failed", %error)
        }
        if let Some(metrics) = self.metrics
            && let Err(error) = metrics.await
        {
            tracing::error!(event = "background_jobs.metrics_join_failed", %error);
        }
        tracing::info!(event = "background_jobs.stopped");
    }
}

fn worker_id() -> String {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "notegate".to_owned());
    format!("{host}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use super::*;

    #[tokio::test]
    async fn reports_an_unexpected_consumer_exit() {
        let consumer = tokio::spawn(async { Ok(()) });
        let reconciler = tokio::spawn(pending::<()>());
        let mut background_jobs = BackgroundJobs {
            consumer: Some(consumer),
            reconciler: Some(reconciler),
            metrics: None,
        };

        let error = background_jobs.wait_for_critical_exit().await;

        assert_eq!(
            error.to_string(),
            "background job consumer stopped unexpectedly"
        );
        assert!(background_jobs.consumer.is_none());
        if let Some(reconciler) = background_jobs.reconciler.take() {
            reconciler.abort();
        }
    }

    #[tokio::test]
    async fn reports_an_unexpected_reconciler_exit() {
        let consumer = tokio::spawn(pending::<JobQueueResult<()>>());
        let reconciler = tokio::spawn(async {});
        let mut background_jobs = BackgroundJobs {
            consumer: Some(consumer),
            reconciler: Some(reconciler),
            metrics: None,
        };

        let error = background_jobs.wait_for_critical_exit().await;

        assert_eq!(
            error.to_string(),
            "background job reconciler stopped unexpectedly"
        );
        assert!(background_jobs.reconciler.is_none());
        if let Some(consumer) = background_jobs.consumer.take() {
            consumer.abort();
        }
    }
}
