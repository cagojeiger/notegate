//! Cloneable runtime wrapper for search execution ownership.

use std::future::Future;
use std::sync::Arc;

use notegate_core::SearchBodyCacheConfig;
use notegate_db::FilesRepo;

use crate::{
    FindPage, FindRequest, GrepPage, GrepRequest, SearchAdmission, SearchCapacity, SearchError,
    SearchResult, SearchService,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SearchRunError {
    #[error("search capacity exhausted for {0:?}")]
    Capacity(SearchCapacity),
    #[error(transparent)]
    Search(#[from] SearchError),
}

impl From<SearchCapacity> for SearchRunError {
    fn from(capacity: SearchCapacity) -> Self {
        Self::Capacity(capacity)
    }
}

pub type SearchRunResult<T> = Result<T, SearchRunError>;

/// Owns the shared search service and process-local admission controls.
#[derive(Debug, Clone)]
pub struct SearchRuntime {
    inner: Arc<SearchRuntimeInner>,
}

#[derive(Debug)]
struct SearchRuntimeInner {
    service: SearchService,
    admission: SearchAdmission,
}

impl SearchRuntime {
    pub fn new(
        store: FilesRepo,
        body_cache_config: SearchBodyCacheConfig,
        metrics_enabled: bool,
    ) -> Self {
        Self::with_authority_and_query_stores(
            store.clone(),
            store,
            body_cache_config,
            metrics_enabled,
        )
    }

    /// Use a primary-backed authority store and an independently scalable query store.
    pub fn with_authority_and_query_stores(
        authority_store: FilesRepo,
        query_store: FilesRepo,
        body_cache_config: SearchBodyCacheConfig,
        metrics_enabled: bool,
    ) -> Self {
        let runtime = Self {
            inner: Arc::new(SearchRuntimeInner {
                service: SearchService::with_authority_and_query_stores(
                    authority_store,
                    query_store,
                    body_cache_config,
                )
                .with_metrics_enabled(metrics_enabled),
                admission: SearchAdmission::default(),
            }),
        };
        runtime.record_body_cache_metrics();
        runtime
    }

    pub async fn find(
        &self,
        caller_account_id: uuid::Uuid,
        space_id: uuid::Uuid,
        request: FindRequest,
    ) -> SearchRunResult<FindPage> {
        run_find(&self.inner.admission, || {
            self.inner
                .service
                .find(caller_account_id, space_id, request)
        })
        .await
    }

    pub async fn grep(
        &self,
        caller_account_id: uuid::Uuid,
        space_id: uuid::Uuid,
        request: GrepRequest,
    ) -> SearchRunResult<GrepPage> {
        run_grep(
            &self.inner.admission,
            || {
                self.inner
                    .service
                    .grep(caller_account_id, space_id, request)
            },
            || self.record_body_cache_metrics(),
        )
        .await
    }

    pub fn record_body_cache_metrics(&self) {
        self.inner.service.record_body_cache_metrics();
    }
}

async fn run_find<T, F>(
    admission: &SearchAdmission,
    execute: impl FnOnce() -> F,
) -> SearchRunResult<T>
where
    F: Future<Output = SearchResult<T>>,
{
    let _permit = admission.enter_find()?;
    execute().await.map_err(SearchRunError::Search)
}

async fn run_grep<T, F>(
    admission: &SearchAdmission,
    execute: impl FnOnce() -> F,
    record_metrics: impl FnOnce(),
) -> SearchRunResult<T>
where
    F: Future<Output = SearchResult<T>>,
{
    let _permit = admission.enter_grep().await?;
    let result = execute().await.map_err(SearchRunError::Search);
    record_metrics();
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};

    use metrics_exporter_prometheus::PrometheusBuilder;
    use notegate_core::SearchBodyCacheConfig;
    use tokio::sync::Notify;

    use super::*;
    use crate::{body_cache::SearchBodyCache, telemetry::SearchTelemetry};

    #[tokio::test]
    async fn runtime_find_reports_capacity_before_querying_store() {
        let admission = SearchAdmission::for_test(0, 1, 1);
        let calls = AtomicUsize::new(0);
        let result = run_find(&admission, || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Ok(()) }
        })
        .await;

        assert!(matches!(
            result,
            Err(SearchRunError::Capacity(SearchCapacity::Find))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn runtime_grep_reports_capacity_before_querying_store() {
        let admission = SearchAdmission::for_test(1, 0, 1);
        let calls = AtomicUsize::new(0);
        let metrics = AtomicUsize::new(0);
        let result = run_grep(
            &admission,
            || {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(()) }
            },
            || {
                metrics.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;

        assert!(matches!(
            result,
            Err(SearchRunError::Capacity(SearchCapacity::Grep))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(metrics.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn runtime_records_body_cache_gauges_when_metrics_are_enabled() {
        let recorder = PrometheusBuilder::new()
            .with_recommended_naming(true)
            .build_recorder();
        let handle = recorder.handle();
        let cache = SearchBodyCache::new(SearchBodyCacheConfig {
            max_capacity_bytes: 128,
            ..SearchBodyCacheConfig::default()
        });
        run_grep(
            &SearchAdmission::for_test(1, 1, 1),
            || async { Ok(()) },
            || {
                metrics::with_local_recorder(&recorder, || {
                    SearchTelemetry::new(true).record_body_cache_metrics(cache.stats());
                })
            },
        )
        .await
        .expect("grep completed");

        let body = handle.render();
        assert!(
            body.contains("notegate_search_body_cache_size_bytes 0"),
            "{body}"
        );
        assert!(
            body.contains("notegate_search_body_cache_capacity_bytes 128"),
            "{body}"
        );
        assert!(
            body.contains("notegate_search_body_cache_entries 0"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn runtime_skips_body_cache_gauges_when_metrics_are_disabled() {
        let recorder = PrometheusBuilder::new()
            .with_recommended_naming(true)
            .build_recorder();
        let handle = recorder.handle();
        run_grep(
            &SearchAdmission::for_test(1, 1, 1),
            || async { Ok(()) },
            || {
                metrics::with_local_recorder(&recorder, || {
                    SearchTelemetry::new(false).record_body_cache_metrics(Default::default());
                })
            },
        )
        .await
        .expect("grep completed");

        assert!(handle.render().is_empty());
    }

    #[tokio::test]
    async fn runtime_find_releases_capacity_after_executor_error() {
        let admission = SearchAdmission::for_test(1, 1, 1);
        let result: SearchRunResult<()> = run_find(&admission, || async {
            Err(SearchError::InvalidInput("injected failure".to_owned()))
        })
        .await;

        assert!(matches!(result, Err(SearchRunError::Search(_))));
        assert!(admission.enter_find().is_ok());
    }

    #[tokio::test]
    async fn runtime_grep_records_metrics_and_releases_capacity_after_executor_error() {
        let admission = SearchAdmission::for_test(1, 1, 1);
        let metrics = AtomicUsize::new(0);
        let result: SearchRunResult<()> = run_grep(
            &admission,
            || async { Err(SearchError::InvalidInput("injected failure".to_owned())) },
            || {
                metrics.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;

        assert!(matches!(result, Err(SearchRunError::Search(_))));
        assert_eq!(metrics.load(Ordering::SeqCst), 1);
        let mut next = std::pin::pin!(admission.enter_grep());
        assert!(matches!(
            next.as_mut().poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(_))
        ));
    }

    #[tokio::test]
    async fn runtime_grep_cancellation_releases_capacity_without_recording_metrics() {
        let admission = SearchAdmission::for_test(1, 1, 1);
        let started = Arc::new(Notify::new());
        let metrics = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn({
            let admission = admission.clone();
            let started = started.clone();
            let metrics = metrics.clone();
            async move {
                run_grep(
                    &admission,
                    || async {
                        started.notify_one();
                        std::future::pending::<SearchResult<()>>().await
                    },
                    || {
                        metrics.fetch_add(1, Ordering::SeqCst);
                    },
                )
                .await
            }
        });
        started.notified().await;
        assert!(matches!(
            admission.enter_grep().await,
            Err(SearchCapacity::Grep)
        ));
        task.abort();
        assert!(task.await.expect_err("task cancelled").is_cancelled());

        let mut next = std::pin::pin!(admission.enter_grep());
        assert!(matches!(
            next.as_mut().poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(_))
        ));
        assert_eq!(metrics.load(Ordering::SeqCst), 0);
    }
}
