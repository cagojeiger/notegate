mod handlers;
mod link_graph;

use handlers::UsageHandler;
use link_graph::LinkGraphProjectNodesHandler;
use notegate_core::BackgroundJobsConfig;
use notegate_db::{LinkGraphProjectNodesJob, PgPool, SpaceUsageReconcileJob, SpaceUsageRepo};
use notegate_jobs::{JobQueue, JobQueueResult, JobRegistry, Worker, WorkerConfig};
use notegate_service::link_graph::LinkGraphService;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub(crate) struct BackgroundJobs {
    consumer: Option<JoinHandle<JobQueueResult<()>>>,
    metrics: Option<JoinHandle<()>>,
    job_kinds: Vec<String>,
}

pub(crate) fn spawn(
    pool: PgPool,
    config: BackgroundJobsConfig,
    metrics_enabled: bool,
    link_graph: LinkGraphService,
    shutdown: CancellationToken,
) -> JobQueueResult<BackgroundJobs> {
    let queue = JobQueue::new(pool.clone());
    let handlers = JobRegistry::new()
        .register::<SpaceUsageReconcileJob>(UsageHandler::new(SpaceUsageRepo::new(pool)))?
        .register::<LinkGraphProjectNodesJob>(LinkGraphProjectNodesHandler::new(link_graph))?;
    let job_kinds = handlers.job_kinds();
    let worker = Worker::new(
        queue.clone(),
        handlers,
        WorkerConfig {
            concurrency: config.concurrency,
            ..WorkerConfig::default()
        },
        worker_id(),
    )?;
    let consumer_shutdown = shutdown.clone();
    let consumer = tokio::spawn(async move { worker.run(consumer_shutdown).await });
    let metrics = crate::observability::spawn_background_job_metrics(
        metrics_enabled,
        queue,
        job_kinds.clone(),
        shutdown,
    );

    tracing::info!(
        event = "background_jobs.started",
        concurrency = config.concurrency
    );
    Ok(BackgroundJobs {
        consumer: Some(consumer),
        metrics,
        job_kinds,
    })
}

impl BackgroundJobs {
    pub(crate) fn job_kinds(&self) -> &[String] {
        &self.job_kinds
    }

    pub(crate) async fn wait_for_critical_exit(&mut self) -> anyhow::Error {
        let result = match self.consumer.as_mut() {
            Some(consumer) => consumer.await,
            None => return anyhow::anyhow!("background job consumer is not running"),
        };
        self.consumer.take();
        match result {
            Ok(Ok(())) => anyhow::anyhow!("background job consumer stopped unexpectedly"),
            Ok(Err(error)) => anyhow::anyhow!(error),
            Err(error) => anyhow::anyhow!("background job consumer task failed: {error}"),
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
    use super::*;

    #[tokio::test]
    async fn reports_an_unexpected_consumer_exit() {
        let consumer = tokio::spawn(async { Ok(()) });
        let mut background_jobs = BackgroundJobs {
            consumer: Some(consumer),
            metrics: None,
            job_kinds: Vec::new(),
        };

        let error = background_jobs.wait_for_critical_exit().await;

        assert_eq!(
            error.to_string(),
            "background job consumer stopped unexpectedly"
        );
        assert!(background_jobs.consumer.is_none());
    }
}
