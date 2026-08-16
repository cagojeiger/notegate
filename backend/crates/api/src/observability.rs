//! Prometheus recorder, scrape endpoint, HTTP RED, and resource utilization metrics.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Extension, State};
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use metrics::{Gauge, Unit};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use notegate_jobs::JobQueue;
use notegate_service::search::SearchBodyCacheStats;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

use crate::state::AppState;

const HTTP_DURATION_BUCKETS_SECONDS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];
const SEARCH_DURATION_BUCKETS_SECONDS: &[f64] = &[
    0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];
const BACKGROUND_EXECUTION_DURATION_BUCKETS_SECONDS: &[f64] = &[
    0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1_200.0, 3_600.0,
];
const UPKEEP_INTERVAL: Duration = Duration::from_secs(5);
const BACKGROUND_JOB_METRICS_INTERVAL: Duration = Duration::from_secs(15);
const BACKGROUND_JOB_STATES: &[&str] = &["ready", "delayed", "running", "lease_expired", "dead"];
const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

#[derive(Clone)]
pub(crate) struct MetricsHandle(PrometheusHandle);

pub(crate) fn install(enabled: bool) -> anyhow::Result<Option<MetricsHandle>> {
    if !enabled {
        return Ok(None);
    }

    let handle = PrometheusBuilder::new()
        .with_recommended_naming(true)
        .set_buckets_for_metric(
            Matcher::Full("notegate_http_request_duration".to_owned()),
            HTTP_DURATION_BUCKETS_SECONDS,
        )?
        .set_buckets_for_metric(
            Matcher::Full("notegate_search_operation_duration".to_owned()),
            SEARCH_DURATION_BUCKETS_SECONDS,
        )?
        .set_buckets_for_metric(
            Matcher::Full("notegate_search_stage_duration".to_owned()),
            SEARCH_DURATION_BUCKETS_SECONDS,
        )?
        .set_buckets_for_metric(
            Matcher::Full("notegate_search_match_reduce_duration".to_owned()),
            SEARCH_DURATION_BUCKETS_SECONDS,
        )?
        .set_buckets_for_metric(
            Matcher::Full("notegate_mcp_tool_duration".to_owned()),
            HTTP_DURATION_BUCKETS_SECONDS,
        )?
        .set_buckets_for_metric(
            Matcher::Full("notegate_db_pool_acquire_duration".to_owned()),
            SEARCH_DURATION_BUCKETS_SECONDS,
        )?
        .set_buckets_for_metric(
            Matcher::Full("notegate_text_decryption_duration".to_owned()),
            SEARCH_DURATION_BUCKETS_SECONDS,
        )?
        .set_buckets_for_metric(
            Matcher::Full("notegate_metadata_write_flush_duration".to_owned()),
            HTTP_DURATION_BUCKETS_SECONDS,
        )?
        .set_buckets_for_metric(
            Matcher::Full("notegate_background_job_duration".to_owned()),
            BACKGROUND_EXECUTION_DURATION_BUCKETS_SECONDS,
        )?
        .set_buckets_for_metric(
            Matcher::Full("notegate_reconciliation_duration".to_owned()),
            BACKGROUND_EXECUTION_DURATION_BUCKETS_SECONDS,
        )?
        .install_recorder()?;

    describe_metrics();

    Ok(Some(MetricsHandle(handle)))
}

fn describe_metrics() {
    metrics::describe_counter!(
        "notegate_http_requests",
        "Completed HTTP requests by normalized route, method, and status class"
    );
    metrics::describe_histogram!(
        "notegate_http_request_duration",
        Unit::Seconds,
        "HTTP request duration in seconds"
    );
    metrics::describe_gauge!(
        "notegate_http_requests_in_flight",
        "HTTP requests currently in flight"
    );
    metrics::describe_gauge!(
        "notegate_db_pool_connections",
        "Database pool connections by bounded state"
    );
    metrics::describe_gauge!(
        "notegate_db_pool_max_connections",
        "Configured maximum database pool connections"
    );
    metrics::describe_gauge!(
        "notegate_search_body_cache_size",
        Unit::Bytes,
        "Approximate weighted size of decrypted search bodies in memory"
    );
    metrics::describe_gauge!(
        "notegate_search_body_cache_capacity",
        Unit::Bytes,
        "Configured maximum weighted size of the decrypted search body cache"
    );
    metrics::describe_gauge!(
        "notegate_search_body_cache_entries",
        "Approximate number of decrypted search bodies in memory"
    );
    metrics::describe_counter!(
        "notegate_search_operations",
        "Completed find and grep operations by bounded mode and outcome"
    );
    metrics::describe_histogram!(
        "notegate_search_operation_duration",
        Unit::Seconds,
        "End-to-end find and grep operation duration"
    );
    metrics::describe_histogram!(
        "notegate_search_stage_duration",
        Unit::Seconds,
        "Search pipeline stage duration by bounded operation and stage"
    );
    metrics::describe_histogram!(
        "notegate_search_match_reduce_duration",
        Unit::Seconds,
        "Search match and reduction duration by bounded operation, mode, and line mode"
    );
    metrics::describe_counter!(
        "notegate_search_candidates",
        "Candidate rows returned to find and grep operations"
    );
    metrics::describe_counter!(
        "notegate_search_results",
        "Nodes returned by successful find and grep operations"
    );
    metrics::describe_counter!(
        "notegate_search_scanned_bytes",
        Unit::Bytes,
        "Plaintext bytes passed to grep content matching"
    );
    metrics::describe_counter!(
        "notegate_search_body_load_bytes",
        Unit::Bytes,
        "Plaintext bytes returned by the grep database body-load boundary"
    );
    metrics::describe_counter!(
        "notegate_search_cache_lookups",
        "Grep decrypted-body cache lookups by bounded result"
    );
    metrics::describe_counter!(
        "notegate_mcp_tool_calls",
        "Completed MCP tool calls by bounded tool and outcome"
    );
    metrics::describe_histogram!(
        "notegate_mcp_tool_duration",
        Unit::Seconds,
        "MCP tool execution duration in seconds"
    );
    metrics::describe_histogram!(
        "notegate_db_pool_acquire_duration",
        Unit::Seconds,
        "SQLx database connection acquisition duration in seconds"
    );
    metrics::describe_counter!(
        "notegate_db_pool_acquire_timeouts",
        "Database connection acquisition timeouts"
    );
    metrics::describe_counter!(
        "notegate_text_decryptions",
        "Server-managed text decryption attempts by bounded boundary and outcome"
    );
    metrics::describe_counter!(
        "notegate_text_decrypted_bytes",
        Unit::Bytes,
        "Plaintext bytes produced by successful server-managed text decryptions"
    );
    metrics::describe_histogram!(
        "notegate_text_decryption_duration",
        Unit::Seconds,
        "Server-managed text decryption duration in seconds"
    );
    metrics::describe_counter!(
        "notegate_metadata_write_flushes",
        "Completed metadata write-behind flush attempts by bounded outcome"
    );
    metrics::describe_histogram!(
        "notegate_metadata_write_flush_duration",
        Unit::Seconds,
        "Metadata write-behind flush duration in seconds"
    );
    metrics::describe_counter!(
        "notegate_metadata_write_items",
        "Metadata write-behind items by bounded kind and disposition"
    );
    metrics::describe_gauge!(
        "notegate_background_jobs",
        "Durable background jobs by bounded kind and state"
    );
    metrics::describe_gauge!(
        "notegate_background_job_oldest_ready_age",
        Unit::Seconds,
        "Age of the oldest ready background job by bounded kind"
    );
    metrics::describe_gauge!(
        "notegate_background_jobs_in_flight",
        "Background job handlers currently running in this process"
    );
    metrics::describe_counter!(
        "notegate_background_job_attempts",
        "Completed background job attempts by kind and outcome"
    );
    metrics::describe_counter!(
        "notegate_background_job_transitions",
        "Background job maintenance transitions"
    );
    metrics::describe_counter!(
        "notegate_background_job_state_transition_errors",
        "Background job state transition persistence errors"
    );
    metrics::describe_histogram!(
        "notegate_background_job_duration",
        Unit::Seconds,
        "Background job attempt wall time including final state persistence"
    );
}

pub(crate) fn record_metadata_flush(enabled: bool, outcome: &'static str, duration: Duration) {
    if !enabled {
        return;
    }
    metrics::counter!("notegate_metadata_write_flushes", "outcome" => outcome).increment(1);
    metrics::histogram!("notegate_metadata_write_flush_duration", "outcome" => outcome)
        .record(duration.as_secs_f64());
}

pub(crate) fn record_metadata_items(
    enabled: bool,
    kind: &'static str,
    disposition: &'static str,
    count: u64,
) {
    if !enabled || count == 0 {
        return;
    }
    metrics::counter!(
        "notegate_metadata_write_items",
        "kind" => kind,
        "disposition" => disposition
    )
    .increment(count);
}

pub(crate) fn record_mcp_tool_metrics(
    enabled: bool,
    tool: &'static str,
    outcome: &'static str,
    duration: Duration,
) {
    if !enabled {
        return;
    }

    metrics::counter!(
        "notegate_mcp_tool_calls",
        "tool" => tool,
        "outcome" => outcome
    )
    .increment(1);
    metrics::histogram!(
        "notegate_mcp_tool_duration",
        "tool" => tool,
        "outcome" => outcome
    )
    .record(duration.as_secs_f64());
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct InternalMetricsLayer;

impl<S> Layer<S> for InternalMetricsLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        match event.metadata().target() {
            "sqlx::pool::acquire" => record_pool_acquire_event(event),
            "notegate_db::pool" => {
                metrics::counter!("notegate_db_pool_acquire_timeouts").increment(1);
            }
            "notegate_db::crypto" => record_text_decryption_event(event),
            _ => {}
        }
    }
}

#[derive(Default)]
struct InternalMetricsVisitor {
    acquire_seconds: Option<f64>,
    duration_seconds: Option<f64>,
    byte_len: Option<u64>,
    boundary: Option<String>,
    outcome: Option<String>,
}

impl Visit for InternalMetricsVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        match field.name() {
            // SQLx 0.8 emits the misspelled `aquired_after_secs` field.
            "aquired_after_secs" | "acquired_after_secs" => self.acquire_seconds = Some(value),
            "duration_seconds" => self.duration_seconds = Some(value),
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "byte_len" {
            self.byte_len = Some(value);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "boundary" => self.boundary = Some(value.to_owned()),
            "outcome" => self.outcome = Some(value.to_owned()),
            _ => {}
        }
    }

    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

fn record_pool_acquire_event(event: &Event<'_>) {
    let mut visitor = InternalMetricsVisitor::default();
    event.record(&mut visitor);
    if let Some(seconds) = visitor.acquire_seconds {
        metrics::histogram!("notegate_db_pool_acquire_duration").record(seconds);
    }
}

fn record_text_decryption_event(event: &Event<'_>) {
    let mut visitor = InternalMetricsVisitor::default();
    event.record(&mut visitor);
    let Some(duration_seconds) = visitor.duration_seconds else {
        return;
    };
    let boundary = match visitor.boundary.as_deref() {
        Some("search_body_load") => "search_body_load",
        _ => "other",
    };
    let outcome = match visitor.outcome.as_deref() {
        Some("success") => "success",
        _ => "error",
    };

    metrics::counter!(
        "notegate_text_decryptions",
        "boundary" => boundary,
        "outcome" => outcome
    )
    .increment(1);
    metrics::histogram!(
        "notegate_text_decryption_duration",
        "boundary" => boundary,
        "outcome" => outcome
    )
    .record(duration_seconds);
    if outcome == "success" {
        metrics::counter!(
            "notegate_text_decrypted_bytes",
            "boundary" => boundary
        )
        .increment(visitor.byte_len.unwrap_or(0));
    }
}

pub(crate) fn spawn_upkeep(
    metrics: Option<MetricsHandle>,
    shutdown: CancellationToken,
) -> Option<JoinHandle<()>> {
    metrics.map(|metrics| {
        tokio::spawn(async move {
            crate::periodic_worker::run(UPKEEP_INTERVAL, shutdown, move || {
                metrics.0.run_upkeep();
                std::future::ready(())
            })
            .await;
        })
    })
}

pub(crate) fn spawn_background_job_metrics(
    enabled: bool,
    queue: JobQueue,
    job_kinds: Vec<String>,
    shutdown: CancellationToken,
) -> Option<JoinHandle<()>> {
    if !enabled {
        return None;
    }
    let job_kinds = Arc::new(job_kinds);
    Some(tokio::spawn(async move {
        crate::periodic_worker::run(BACKGROUND_JOB_METRICS_INTERVAL, shutdown, move || {
            let queue = queue.clone();
            let job_kinds = job_kinds.clone();
            async move {
                if let Err(error) = refresh_background_job_metrics(&queue, &job_kinds).await {
                    tracing::error!(event = "background_jobs.metrics_refresh_failed", %error);
                }
            }
        })
        .await;
    }))
}

async fn refresh_background_job_metrics(
    queue: &JobQueue,
    job_kinds: &[String],
) -> notegate_jobs::JobQueueResult<()> {
    let snapshot = queue.snapshot(job_kinds).await?;
    for kind in job_kinds {
        for state in BACKGROUND_JOB_STATES {
            metrics::gauge!(
                "notegate_background_jobs",
                "kind" => kind.clone(),
                "state" => *state,
            )
            .set(0.0);
        }
        metrics::gauge!(
            "notegate_background_job_oldest_ready_age",
            "kind" => kind.clone(),
        )
        .set(0.0);
    }
    for count in snapshot.states {
        metrics::gauge!(
            "notegate_background_jobs",
            "kind" => count.kind,
            "state" => count.state,
        )
        .set(count.count as f64);
    }
    let now = chrono::Utc::now();
    for oldest in snapshot.oldest_ready {
        let age = now
            .signed_duration_since(oldest.available_at)
            .num_milliseconds()
            .max(0) as f64
            / 1_000.0;
        metrics::gauge!(
            "notegate_background_job_oldest_ready_age",
            "kind" => oldest.kind,
        )
        .set(age);
    }
    Ok(())
}

pub(crate) fn routes(metrics: MetricsHandle) -> Router<AppState> {
    Router::new()
        .route("/metrics", get(scrape))
        .layer(Extension(metrics))
}

async fn scrape(
    State(state): State<AppState>,
    Extension(metrics): Extension<MetricsHandle>,
) -> Response {
    record_resource_metrics(ResourceMetricsSnapshot::capture(&state));
    scrape_response(&metrics)
}

fn scrape_response(metrics: &MetricsHandle) -> Response {
    metrics.0.run_upkeep();
    (
        [(header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)],
        metrics.0.render(),
    )
        .into_response()
}

#[derive(Debug, Clone, Copy)]
struct ResourceMetricsSnapshot {
    db_connections_in_use: u32,
    db_connections_idle: u32,
    db_max_connections: u32,
    body_cache: SearchBodyCacheStats,
}

impl ResourceMetricsSnapshot {
    fn capture(state: &AppState) -> Self {
        let db_connections = state.db.size();
        let db_connections_idle = u32::try_from(state.db.num_idle())
            .unwrap_or(u32::MAX)
            .min(db_connections);

        Self {
            db_connections_in_use: db_connections.saturating_sub(db_connections_idle),
            db_connections_idle,
            db_max_connections: state.config.db_max_connections,
            body_cache: state.search.body_cache_stats(),
        }
    }
}

fn record_resource_metrics(snapshot: ResourceMetricsSnapshot) {
    metrics::gauge!(
        "notegate_db_pool_connections",
        "state" => "in_use"
    )
    .set(f64::from(snapshot.db_connections_in_use));
    metrics::gauge!(
        "notegate_db_pool_connections",
        "state" => "idle"
    )
    .set(f64::from(snapshot.db_connections_idle));
    metrics::gauge!("notegate_db_pool_max_connections").set(f64::from(snapshot.db_max_connections));
    metrics::gauge!("notegate_search_body_cache_size").set(snapshot.body_cache.size_bytes as f64);
    metrics::gauge!("notegate_search_body_cache_capacity")
        .set(snapshot.body_cache.capacity_bytes as f64);
    metrics::gauge!("notegate_search_body_cache_entries").set(snapshot.body_cache.entries as f64);
}

pub(crate) struct HttpRequestMetrics {
    method: &'static str,
    route: String,
    in_flight: Gauge,
}

impl HttpRequestMetrics {
    pub(crate) fn start(method: &Method, route: &str) -> Self {
        let method = method_label(method);
        let route = route_label(route).to_owned();
        let in_flight = metrics::gauge!(
            "notegate_http_requests_in_flight",
            "method" => method,
            "route" => route.clone(),
        );
        in_flight.increment(1.0);

        Self {
            method,
            route,
            in_flight,
        }
    }

    pub(crate) fn finish(self, status: StatusCode, latency: Duration) {
        let status_class = status_class(status);
        metrics::counter!(
            "notegate_http_requests",
            "method" => self.method,
            "route" => self.route.clone(),
            "status_class" => status_class,
        )
        .increment(1);
        metrics::histogram!(
            "notegate_http_request_duration",
            "method" => self.method,
            "route" => self.route.clone(),
            "status_class" => status_class,
        )
        .record(latency.as_secs_f64());
    }
}

impl Drop for HttpRequestMetrics {
    fn drop(&mut self) {
        self.in_flight.decrement(1.0);
    }
}

fn method_label(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::HEAD => "HEAD",
        Method::OPTIONS => "OPTIONS",
        Method::CONNECT => "CONNECT",
        Method::TRACE => "TRACE",
        _ => "OTHER",
    }
}

fn route_label(route: &str) -> &str {
    if route.is_empty() { "unmatched" } else { route }
}

fn status_class(status: StatusCode) -> &'static str {
    match status.as_u16() / 100 {
        1 => "1xx",
        2 => "2xx",
        3 => "3xx",
        4 => "4xx",
        5 => "5xx",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use axum::body::to_bytes;
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::*;

    fn test_metrics() -> (
        MetricsHandle,
        metrics_exporter_prometheus::PrometheusRecorder,
    ) {
        let recorder = PrometheusBuilder::new()
            .with_recommended_naming(true)
            .set_buckets_for_metric(
                Matcher::Full("notegate_http_request_duration".to_owned()),
                HTTP_DURATION_BUCKETS_SECONDS,
            )
            .unwrap()
            .set_buckets_for_metric(
                Matcher::Full("notegate_search_operation_duration".to_owned()),
                SEARCH_DURATION_BUCKETS_SECONDS,
            )
            .unwrap()
            .set_buckets_for_metric(
                Matcher::Full("notegate_search_stage_duration".to_owned()),
                SEARCH_DURATION_BUCKETS_SECONDS,
            )
            .unwrap()
            .set_buckets_for_metric(
                Matcher::Full("notegate_search_match_reduce_duration".to_owned()),
                SEARCH_DURATION_BUCKETS_SECONDS,
            )
            .unwrap()
            .set_buckets_for_metric(
                Matcher::Full("notegate_mcp_tool_duration".to_owned()),
                HTTP_DURATION_BUCKETS_SECONDS,
            )
            .unwrap()
            .set_buckets_for_metric(
                Matcher::Full("notegate_db_pool_acquire_duration".to_owned()),
                SEARCH_DURATION_BUCKETS_SECONDS,
            )
            .unwrap()
            .set_buckets_for_metric(
                Matcher::Full("notegate_text_decryption_duration".to_owned()),
                SEARCH_DURATION_BUCKETS_SECONDS,
            )
            .unwrap()
            .set_buckets_for_metric(
                Matcher::Full("notegate_metadata_write_flush_duration".to_owned()),
                HTTP_DURATION_BUCKETS_SECONDS,
            )
            .unwrap()
            .set_buckets_for_metric(
                Matcher::Full("notegate_background_job_duration".to_owned()),
                BACKGROUND_EXECUTION_DURATION_BUCKETS_SECONDS,
            )
            .unwrap()
            .set_buckets_for_metric(
                Matcher::Full("notegate_reconciliation_duration".to_owned()),
                BACKGROUND_EXECUTION_DURATION_BUCKETS_SECONDS,
            )
            .unwrap()
            .build_recorder();
        (MetricsHandle(recorder.handle()), recorder)
    }

    #[test]
    fn request_labels_are_bounded() {
        assert_eq!(method_label(&Method::GET), "GET");
        assert_eq!(
            method_label(&Method::from_bytes(b"CUSTOM").unwrap()),
            "OTHER"
        );
        assert_eq!(route_label(""), "unmatched");
        assert_eq!(
            route_label("/api/v1/spaces/{space_id}"),
            "/api/v1/spaces/{space_id}"
        );
        assert_eq!(status_class(StatusCode::OK), "2xx");
        assert_eq!(status_class(StatusCode::INTERNAL_SERVER_ERROR), "5xx");
    }

    #[tokio::test]
    async fn scrape_renders_prometheus_text() {
        let (handle, recorder) = test_metrics();
        metrics::with_local_recorder(&recorder, || {
            describe_metrics();
            metrics::describe_histogram!(
                "notegate_reconciliation_duration",
                Unit::Seconds,
                "Reconciliation handler duration"
            );
            HttpRequestMetrics::start(&Method::GET, "/health")
                .finish(StatusCode::OK, Duration::from_millis(10));
            record_resource_metrics(ResourceMetricsSnapshot {
                db_connections_in_use: 3,
                db_connections_idle: 7,
                db_max_connections: 20,
                body_cache: SearchBodyCacheStats {
                    entries: 4,
                    size_bytes: 64,
                    capacity_bytes: 128,
                },
            });
            metrics::counter!(
                "notegate_search_operations",
                "operation" => "grep",
                "mode" => "regex",
                "outcome" => "success",
            )
            .increment(1);
            metrics::histogram!(
                "notegate_search_operation_duration",
                "operation" => "grep",
                "mode" => "regex",
                "outcome" => "success",
            )
            .record(0.01);
            metrics::histogram!(
                "notegate_search_stage_duration",
                "operation" => "grep",
                "stage" => "candidate_query",
            )
            .record(0.005);
            metrics::histogram!(
                "notegate_search_match_reduce_duration",
                "operation" => "grep",
                "mode" => "regex",
                "line_mode" => "all",
            )
            .record(0.004);
            metrics::counter!("notegate_search_candidates", "operation" => "grep").increment(4);
            metrics::counter!("notegate_search_results", "operation" => "grep").increment(2);
            metrics::counter!("notegate_search_scanned_bytes", "operation" => "grep").increment(64);
            metrics::counter!("notegate_search_body_load_bytes", "operation" => "grep")
                .increment(32);
            metrics::counter!("notegate_search_cache_lookups", "result" => "hit").increment(3);
            metrics::counter!(
                "notegate_mcp_tool_calls",
                "tool" => "search",
                "outcome" => "success"
            )
            .increment(1);
            metrics::histogram!(
                "notegate_mcp_tool_duration",
                "tool" => "search",
                "outcome" => "success"
            )
            .record(0.015);
            record_metadata_flush(true, "success", Duration::from_millis(2));
            record_metadata_items(true, "api_key", "flushed", 3);
            metrics::histogram!(
                "notegate_background_job_duration",
                "kind" => "space_usage_reconcile",
            )
            .record(0.5);
            metrics::histogram!(
                "notegate_reconciliation_duration",
                "kind" => "system.purge",
                "outcome" => "succeeded",
            )
            .record(1.0);

            let subscriber = tracing_subscriber::registry().with(InternalMetricsLayer);
            tracing::subscriber::with_default(subscriber, || {
                tracing::trace!(
                    target: "sqlx::pool::acquire",
                    aquired_after_secs = 0.002,
                    "acquired connection"
                );
                tracing::trace!(
                    target: "notegate_db::crypto",
                    event = "text.decrypt",
                    boundary = "search_body_load",
                    outcome = "success",
                    byte_len = 128_u64,
                    duration_seconds = 0.001,
                );
                tracing::warn!(
                    target: "notegate_db::pool",
                    event = "acquire.timeout",
                    "database pool acquisition timed out"
                );
            });
        });

        let response = scrape_response(&handle);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            PROMETHEUS_CONTENT_TYPE
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("notegate_http_requests_total"));
        assert!(body.contains("method=\"GET\""));
        assert!(body.contains("route=\"/health\""));
        assert!(body.contains("status_class=\"2xx\""));
        assert!(body.contains("notegate_http_request_duration_seconds_bucket"));
        assert!(body.contains("notegate_http_requests_in_flight"));
        assert!(body.contains("notegate_db_pool_connections{state=\"in_use\"} 3"));
        assert!(body.contains("notegate_db_pool_connections{state=\"idle\"} 7"));
        assert!(body.contains("notegate_db_pool_max_connections 20"));
        assert!(body.contains("notegate_search_body_cache_size_bytes 64"));
        assert!(body.contains("notegate_search_body_cache_capacity_bytes 128"));
        assert!(body.contains("notegate_search_body_cache_entries 4"));
        assert!(body.contains(
            "notegate_search_operations_total{operation=\"grep\",mode=\"regex\",outcome=\"success\"} 1"
        ));
        assert!(body.contains("notegate_search_operation_duration_seconds_bucket"));
        assert!(body.contains("notegate_search_stage_duration_seconds_bucket"));
        assert!(body.contains(
            "notegate_search_match_reduce_duration_seconds_bucket{operation=\"grep\",mode=\"regex\",line_mode=\"all\""
        ));
        assert!(body.contains("notegate_search_candidates_total{operation=\"grep\"} 4"));
        assert!(body.contains("notegate_search_results_total{operation=\"grep\"} 2"));
        assert!(body.contains("notegate_search_scanned_bytes_total{operation=\"grep\"} 64"));
        assert!(body.contains("notegate_search_body_load_bytes_total{operation=\"grep\"} 32"));
        assert!(body.contains("notegate_search_cache_lookups_total{result=\"hit\"} 3"));
        assert!(
            body.contains("notegate_mcp_tool_calls_total{tool=\"search\",outcome=\"success\"} 1")
        );
        assert!(body.contains("notegate_mcp_tool_duration_seconds_bucket"));
        assert!(body.contains("notegate_db_pool_acquire_duration_seconds_bucket"));
        assert!(body.contains("notegate_db_pool_acquire_timeouts_total 1"));
        assert!(body.contains(
            "notegate_text_decryptions_total{boundary=\"search_body_load\",outcome=\"success\"} 1"
        ));
        assert!(
            body.contains("notegate_text_decrypted_bytes_total{boundary=\"search_body_load\"} 128")
        );
        assert!(body.contains("notegate_text_decryption_duration_seconds_bucket"));
        assert!(body.contains("notegate_metadata_write_flushes_total{outcome=\"success\"} 1"));
        assert!(body.contains("notegate_metadata_write_flush_duration_seconds_bucket"));
        assert!(body.contains("notegate_background_job_duration_seconds_bucket"));
        assert!(body.contains("notegate_reconciliation_duration_seconds_bucket"));
        assert!(body.contains(
            "notegate_metadata_write_items_total{kind=\"api_key\",disposition=\"flushed\"} 3"
        ));
    }
}
