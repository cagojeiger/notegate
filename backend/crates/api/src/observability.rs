//! Prometheus recorder, scrape endpoint, and bounded HTTP RED metrics.

use std::time::Duration;

use axum::Router;
use axum::extract::Extension;
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use metrics::{Gauge, Unit};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const HTTP_DURATION_BUCKETS_SECONDS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];
const UPKEEP_INTERVAL: Duration = Duration::from_secs(5);
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

pub(crate) fn routes<S>(metrics: MetricsHandle) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/metrics", get(scrape))
        .layer(Extension(metrics))
}

async fn scrape(Extension(metrics): Extension<MetricsHandle>) -> Response {
    metrics.0.run_upkeep();
    (
        [(header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)],
        metrics.0.render(),
    )
        .into_response()
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

    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt as _;

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
            HttpRequestMetrics::start(&Method::GET, "/health")
                .finish(StatusCode::OK, Duration::from_millis(10));
        });

        let response = routes::<()>(handle)
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

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
    }
}
