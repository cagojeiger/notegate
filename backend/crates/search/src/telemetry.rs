use std::future::Future;
use std::time::{Duration, Instant};

use crate::{SearchError, SearchResult};

#[derive(Debug, Clone, Copy)]
pub(super) enum SearchOperation {
    Find,
    Grep,
}

impl SearchOperation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Find => "find",
            Self::Grep => "grep",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum SearchStage {
    Authorize,
    Prepare,
    ResolveScope,
    CandidateQuery,
    CacheLookup,
    BodyLoad,
    MatchReduce,
    Hydrate,
}

impl SearchStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Authorize => "authorize",
            Self::Prepare => "prepare",
            Self::ResolveScope => "resolve_scope",
            Self::CandidateQuery => "candidate_query",
            Self::CacheLookup => "cache_lookup",
            Self::BodyLoad => "body_load",
            Self::MatchReduce => "match_reduce",
            Self::Hydrate => "hydrate",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum CacheResult {
    Hit,
    Miss,
    Coalesced,
}

impl CacheResult {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Coalesced => "coalesced",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SearchTelemetry {
    enabled: bool,
}

impl SearchTelemetry {
    pub(super) const fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    pub(super) fn operation(
        self,
        operation: SearchOperation,
        mode: &'static str,
    ) -> SearchOperationTimer {
        SearchOperationTimer {
            operation,
            mode,
            started: self.enabled.then(Instant::now),
        }
    }

    pub(super) async fn stage<T>(
        self,
        operation: SearchOperation,
        stage: SearchStage,
        future: impl Future<Output = T>,
    ) -> T {
        if !self.enabled {
            return future.await;
        }

        let started = Instant::now();
        let result = future.await;
        record_stage(operation, stage, started.elapsed());
        result
    }

    pub(super) fn stage_sync<T>(
        self,
        operation: SearchOperation,
        stage: SearchStage,
        work: impl FnOnce() -> T,
    ) -> T {
        if !self.enabled {
            return work();
        }

        let started = Instant::now();
        let result = work();
        record_stage(operation, stage, started.elapsed());
        result
    }

    pub(super) fn match_reduce<T>(
        self,
        operation: SearchOperation,
        mode: &'static str,
        line_mode: Option<&'static str>,
        work: impl FnOnce() -> T,
    ) -> T {
        if !self.enabled {
            return work();
        }

        let started = Instant::now();
        let result = work();
        let elapsed = started.elapsed();
        record_stage(operation, SearchStage::MatchReduce, elapsed);
        metrics::histogram!(
            "notegate_search_match_reduce_duration",
            "operation" => operation.as_str(),
            "mode" => mode,
            "line_mode" => line_mode.unwrap_or("not_applicable"),
        )
        .record(elapsed.as_secs_f64());
        result
    }

    pub(super) fn record_workload(
        self,
        operation: SearchOperation,
        candidates: usize,
        results: usize,
        scanned_bytes: usize,
        body_load_bytes: usize,
    ) {
        if !self.enabled {
            return;
        }

        let operation = operation.as_str();
        metrics::counter!("notegate_search_candidates", "operation" => operation)
            .increment(saturating_u64(candidates));
        metrics::counter!("notegate_search_results", "operation" => operation)
            .increment(saturating_u64(results));
        metrics::counter!("notegate_search_scanned_bytes", "operation" => operation)
            .increment(saturating_u64(scanned_bytes));
        metrics::counter!("notegate_search_body_load_bytes", "operation" => operation)
            .increment(saturating_u64(body_load_bytes));
    }

    pub(super) fn record_cache(self, result: CacheResult, count: usize) {
        if !self.enabled || count == 0 {
            return;
        }

        metrics::counter!(
            "notegate_search_cache_lookups",
            "result" => result.as_str()
        )
        .increment(saturating_u64(count));
    }
}

pub(super) struct SearchOperationTimer {
    operation: SearchOperation,
    mode: &'static str,
    started: Option<Instant>,
}

impl SearchOperationTimer {
    pub(super) fn finish<T>(self, result: &SearchResult<T>) {
        let Some(started) = self.started else {
            return;
        };

        let operation = self.operation.as_str();
        let outcome = outcome_label(result);
        metrics::counter!(
            "notegate_search_operations",
            "operation" => operation,
            "mode" => self.mode,
            "outcome" => outcome,
        )
        .increment(1);
        metrics::histogram!(
            "notegate_search_operation_duration",
            "operation" => operation,
            "mode" => self.mode,
            "outcome" => outcome,
        )
        .record(started.elapsed().as_secs_f64());
    }
}

fn record_stage(operation: SearchOperation, stage: SearchStage, elapsed: Duration) {
    metrics::histogram!(
        "notegate_search_stage_duration",
        "operation" => operation.as_str(),
        "stage" => stage.as_str(),
    )
    .record(elapsed.as_secs_f64());
}

fn outcome_label<T>(result: &SearchResult<T>) -> &'static str {
    match result {
        Ok(_) => "success",
        Err(SearchError::InvalidInput(_)) => "invalid",
        Err(SearchError::NotFound(_)) => "not_found",
        Err(SearchError::Forbidden(_)) => "forbidden",
        Err(
            SearchError::Conflict(_)
            | SearchError::WriteLocked { .. }
            | SearchError::UsageRecalculationInProgress { .. },
        ) => "conflict",
        Err(SearchError::Internal(_)) => "internal",
    }
}

fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

    use super::*;

    fn recorder() -> (
        PrometheusHandle,
        metrics_exporter_prometheus::PrometheusRecorder,
    ) {
        let recorder = PrometheusBuilder::new()
            .with_recommended_naming(true)
            .build_recorder();
        (recorder.handle(), recorder)
    }

    #[test]
    fn outcome_labels_are_bounded() {
        assert_eq!(outcome_label(&Ok::<(), SearchError>(())), "success");
        assert_eq!(
            outcome_label(&Err::<(), _>(SearchError::InvalidInput(
                "ignored".to_owned()
            ))),
            "invalid"
        );
        assert_eq!(
            outcome_label(&Err::<(), _>(SearchError::Internal("ignored".to_owned()))),
            "internal"
        );
    }

    #[test]
    fn enabled_telemetry_records_only_bounded_labels() {
        let (handle, recorder) = recorder();
        metrics::with_local_recorder(&recorder, || {
            let telemetry = SearchTelemetry::new(true);
            let timer = telemetry.operation(SearchOperation::Grep, "regex");
            telemetry.match_reduce(SearchOperation::Grep, "regex", Some("all"), || ());
            telemetry.match_reduce(SearchOperation::Find, "glob", None, || ());
            telemetry.record_workload(SearchOperation::Grep, 4, 2, 64, 32);
            telemetry.record_cache(CacheResult::Coalesced, 1);
            timer.finish(&Ok::<(), SearchError>(()));
        });

        let body = handle.render();
        assert!(
            body.contains(
                "notegate_search_operations_total{operation=\"grep\",mode=\"regex\",outcome=\"success\"} 1"
            ),
            "{body}"
        );
        assert!(body.contains("notegate_search_candidates_total{operation=\"grep\"} 4"));
        assert!(body.contains("notegate_search_results_total{operation=\"grep\"} 2"));
        assert!(body.contains("notegate_search_scanned_bytes_total{operation=\"grep\"} 64"));
        assert!(body.contains("notegate_search_body_load_bytes_total{operation=\"grep\"} 32"));
        assert!(body.contains("notegate_search_cache_lookups_total{result=\"coalesced\"} 1"));
        assert!(
            body.contains(
                "notegate_search_match_reduce_duration{operation=\"grep\",mode=\"regex\",line_mode=\"all\""
            ),
            "{body}"
        );
        assert!(
            body.contains(
                "notegate_search_match_reduce_duration{operation=\"find\",mode=\"glob\",line_mode=\"not_applicable\""
            ),
            "{body}"
        );
    }

    #[test]
    fn disabled_telemetry_records_nothing() {
        let (handle, recorder) = recorder();
        metrics::with_local_recorder(&recorder, || {
            let telemetry = SearchTelemetry::new(false);
            let timer = telemetry.operation(SearchOperation::Find, "contains");
            telemetry.match_reduce(SearchOperation::Find, "contains", None, || ());
            telemetry.record_workload(SearchOperation::Find, 4, 2, 0, 0);
            telemetry.record_cache(CacheResult::Hit, 1);
            timer.finish(&Ok::<(), SearchError>(()));
        });

        assert!(handle.render().is_empty());
    }
}
