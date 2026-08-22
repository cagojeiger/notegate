//! Cloneable runtime wrapper for search execution ownership.

use std::sync::Arc;

use notegate_core::SearchBodyCacheConfig;
use notegate_db::FilesRepo;

use crate::{
    FindPage, FindRequest, GrepPage, GrepRequest, SearchAdmission, SearchCapacity, SearchError,
    SearchService,
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
        let _permit = self.inner.admission.enter_find()?;
        self.inner
            .service
            .find(caller_account_id, space_id, request)
            .await
            .map_err(SearchRunError::Search)
    }

    pub async fn grep(
        &self,
        caller_account_id: uuid::Uuid,
        space_id: uuid::Uuid,
        request: GrepRequest,
    ) -> SearchRunResult<GrepPage> {
        let _permit = self.inner.admission.enter_grep().await?;
        let result = self
            .inner
            .service
            .grep(caller_account_id, space_id, request)
            .await
            .map_err(SearchRunError::Search);
        self.record_body_cache_metrics();
        result
    }

    pub fn record_body_cache_metrics(&self) {
        self.inner.service.record_body_cache_metrics();
    }

    #[cfg(test)]
    fn with_admission(service: SearchService, admission: SearchAdmission) -> Self {
        Self {
            inner: Arc::new(SearchRuntimeInner { service, admission }),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use metrics_exporter_prometheus::PrometheusBuilder;
    use notegate_core::{SearchBodyCacheConfig, limits::Limits, security::PiiCrypto};

    use super::*;
    use crate::{FindMatchMode, GrepLineMode, GrepMatchMode};

    fn lazy_store() -> FilesRepo {
        let pool =
            notegate_db::PgPool::connect_lazy("postgres://notegate:notegate@127.0.0.1:1/notegate")
                .expect("lazy pool");
        FilesRepo::with_limits_and_crypto(pool, Limits::default(), PiiCrypto::test())
    }

    fn find_request() -> FindRequest {
        FindRequest {
            q: "note".to_owned(),
            path: None,
            kind: None,
            match_mode: FindMatchMode::Contains,
            include: Vec::new(),
            exclude: Vec::new(),
            limit: None,
            cursor: None,
        }
    }

    fn grep_request() -> GrepRequest {
        GrepRequest {
            q: "needle".to_owned(),
            path: None,
            match_mode: GrepMatchMode::Literal,
            line_mode: GrepLineMode::First,
            include: Vec::new(),
            exclude: Vec::new(),
            limit: None,
            cursor: None,
        }
    }

    #[tokio::test]
    async fn runtime_find_reports_capacity_before_querying_store() {
        let runtime = SearchRuntime::with_admission(
            SearchService::new(lazy_store()),
            SearchAdmission::for_test(0, 1, 1),
        );

        let result = runtime
            .find(uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), find_request())
            .await;

        assert!(matches!(
            result,
            Err(SearchRunError::Capacity(SearchCapacity::Find))
        ));
    }

    #[tokio::test]
    async fn runtime_grep_reports_capacity_before_querying_store() {
        let runtime = SearchRuntime::with_admission(
            SearchService::new(lazy_store()),
            SearchAdmission::for_test(1, 0, 1),
        );

        let result = runtime
            .grep(uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), grep_request())
            .await;

        assert!(matches!(
            result,
            Err(SearchRunError::Capacity(SearchCapacity::Grep))
        ));
    }

    #[tokio::test]
    async fn runtime_records_body_cache_gauges_when_metrics_are_enabled() {
        let recorder = PrometheusBuilder::new()
            .with_recommended_naming(true)
            .build_recorder();
        let handle = recorder.handle();
        let runtime = SearchRuntime::new(
            lazy_store(),
            SearchBodyCacheConfig {
                max_capacity_bytes: 128,
                ..SearchBodyCacheConfig::default()
            },
            true,
        );

        metrics::with_local_recorder(&recorder, || runtime.record_body_cache_metrics());

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
        let runtime = SearchRuntime::new(
            lazy_store(),
            SearchBodyCacheConfig {
                max_capacity_bytes: 128,
                ..SearchBodyCacheConfig::default()
            },
            false,
        );

        metrics::with_local_recorder(&recorder, || runtime.record_body_cache_metrics());

        assert!(handle.render().is_empty());
    }
}
