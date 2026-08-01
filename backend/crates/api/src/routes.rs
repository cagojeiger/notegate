//! Router assembly and HTTP handlers.

use std::path::Path;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{MatchedPath, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderName};
use axum::http::{HeaderValue, Method, Request, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::Response;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use axum_governor::extractor::Global;
use axum_governor::{GovernorConfigBuilder, GovernorLayer, Quota};
use notegate_core::{HttpRateLimitConfig, HttpRateLimitsConfig, limits};
use serde::Serialize;
use tower::ServiceBuilder;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::timeout::TimeoutLayer;
use tracing::{Instrument, debug, error, info, info_span, warn};

use crate::auth::metadata::{
    authorization_server_metadata, protected_resource_metadata, protected_resource_metadata_url,
};
use crate::auth::oauth::{callback, login, logout, success};
use crate::auth::{mark_private_no_store, require_browser_session, require_public_api_key};
use crate::error::ApiError;
use crate::mcp::server::mcp_handler;
use crate::observability::{self, HttpRequestMetrics, MetricsHandle};
use crate::state::AppState;

pub fn app(state: AppState) -> Router {
    let x_request_id = HeaderName::from_static("x-request-id");
    let metrics = state.metrics.clone();
    let metrics_enabled = metrics.is_some();

    with_web_fallback(
        Router::new()
            .merge(control_plane_routes(metrics))
            .merge(data_plane_routes(state.clone())),
        state.config.web_dist_dir.as_deref(),
    )
    .with_state(state)
    .layer(
        ServiceBuilder::new()
            .layer(SetRequestIdLayer::new(
                x_request_id.clone(),
                MakeRequestUuid,
            ))
            .layer(from_fn_with_state(metrics_enabled, log_request))
            .layer(from_fn(add_json_charset))
            .layer(PropagateRequestIdLayer::new(x_request_id)),
    )
}

#[derive(Debug, Clone, Copy)]
struct DataPlaneLimits {
    request_body_max_bytes: usize,
    request_timeout: Duration,
    rate_limits: HttpRateLimitsConfig,
}

impl Default for DataPlaneLimits {
    fn default() -> Self {
        Self {
            request_body_max_bytes: limits::HTTP_REQUEST_BODY_MAX_BYTES,
            request_timeout: Duration::from_secs(limits::HTTP_REQUEST_TIMEOUT_SECS),
            rate_limits: HttpRateLimitsConfig::default(),
        }
    }
}

fn with_web_fallback<S>(router: Router<S>, web_dist_dir: Option<&str>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    match web_dist_dir {
        Some(dir) => {
            let assets = Router::<()>::new()
                .fallback_service(ServeDir::new(Path::new(dir).join("assets")))
                .layer(from_fn(set_asset_cache_headers));
            let web = Router::<()>::new()
                .fallback_service(
                    ServeDir::new(dir).fallback(ServeFile::new(format!("{dir}/index.html"))),
                )
                .layer(from_fn(set_html_revalidation));

            router.nest_service("/assets", assets).fallback_service(web)
        }
        None => router,
    }
}

async fn set_asset_cache_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    if response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED {
        response.headers_mut().insert(
            CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    response
}

async fn set_html_revalidation(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    if response
        .headers()
        .get(CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes().starts_with(b"text/html"))
    {
        response.headers_mut().insert(
            CACHE_CONTROL,
            HeaderValue::from_static("no-cache, must-revalidate"),
        );
    }
    response
}

fn control_plane_routes(metrics: Option<MetricsHandle>) -> Router<AppState> {
    let routes = match metrics {
        Some(metrics) => system_routes().merge(observability::routes(metrics)),
        None => system_routes(),
    };
    apply_control_plane_limits(routes)
}

fn data_plane_routes(state: AppState) -> Router<AppState> {
    let limits = DataPlaneLimits {
        rate_limits: state.config.http_rate_limits,
        ..DataPlaneLimits::default()
    };
    let browser_v1 = apply_rate_limit(
        Router::new().nest("/api", rest_api_routes(state.clone())),
        limits.rate_limits.browser_v1,
    );
    let public_v2 = apply_rate_limit(
        Router::new().nest("/api/v2", public_v2_routes(state.clone())),
        limits.rate_limits.public_v2,
    );
    let mcp = apply_rate_limit(
        Router::new().route("/mcp", any(mcp_handler)),
        limits.rate_limits.mcp,
    );
    let router = Router::new()
        .merge(auth_routes())
        .merge(metadata_routes(&state))
        .merge(crate::openapi::routes(&state.config))
        .merge(browser_v1)
        .merge(public_v2)
        .merge(mcp);
    apply_data_plane_limits(router, limits)
}

fn apply_control_plane_limits<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(limits::HTTP_CONTROL_PLANE_TIMEOUT_SECS),
    ))
}

fn apply_data_plane_limits<S>(router: Router<S>, limits: DataPlaneLimits) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(
        ServiceBuilder::new()
            .layer(RequestBodyLimitLayer::new(limits.request_body_max_bytes))
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                limits.request_timeout,
            ))
            .layer(GovernorLayer::new(rate_limit_config(
                limits.rate_limits.ingress,
            ))),
    )
}

fn apply_rate_limit<S>(router: Router<S>, limit: HttpRateLimitConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(GovernorLayer::new(rate_limit_config(limit)))
}

#[allow(clippy::expect_used)]
fn rate_limit_config(limit: HttpRateLimitConfig) -> axum_governor::GovernorConfig<()> {
    let requests = std::num::NonZeroU32::new(limit.requests_per_second)
        .expect("HTTP rate limit must be greater than zero");
    let burst = std::num::NonZeroU32::new(limit.burst)
        .expect("HTTP rate limit burst must be greater than zero");
    GovernorConfigBuilder::default()
        .with_extractor(Global)
        .quota_default(Quota::requests_per_second(requests).burst(burst))
        .finish()
        .expect("global HTTP rate limit config is statically valid")
}

fn system_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
}

fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/success", get(success))
        .route("/auth/logout", post(logout))
}

fn metadata_routes(state: &AppState) -> Router<AppState> {
    let metadata_path = protected_resource_metadata_url(&state.config.resource_url).route_path;
    let wildcard_path = format!("{metadata_path}/{{*path}}");
    let router = Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        );

    if metadata_path == "/.well-known/oauth-protected-resource" {
        router.route(&wildcard_path, get(protected_resource_metadata))
    } else {
        router
            .route(&metadata_path, get(protected_resource_metadata))
            .route(&wildcard_path, get(protected_resource_metadata))
    }
}

fn rest_api_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .merge(crate::rest::me::routes())
        .merge(crate::rest::spaces::routes())
        .merge(crate::rest::nodes::routes())
        .merge(crate::rest::text::routes())
        .merge(crate::rest::file_uploads::routes())
        .merge(crate::rest::files::routes())
        .merge(crate::rest::connections::routes())
        .merge(crate::rest::agents::routes())
        .fallback(api_not_found)
        .layer(from_fn_with_state(state, require_browser_session))
}

fn public_v2_routes(state: AppState) -> Router<AppState> {
    crate::public_v2::routes()
        .fallback(api_not_found)
        .layer(from_fn_with_state(state, require_public_api_key))
        .layer(from_fn(mark_private_no_store))
}

/// Liveness: the process is up. No dependency checks.
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Readiness: verify the database and embedded migrations before reporting ready.
async fn ready(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    notegate_db::check_readiness(&state.db)
        .await
        .map_err(|error| {
            tracing::error!(event = "ready.failed", %error);
            ApiError::internal("database not ready")
        })?;

    Ok(Json(HealthResponse { status: "ready" }))
}

async fn api_not_found() -> ApiError {
    ApiError::not_found("api route not found")
}

async fn add_json_charset(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let is_json = response
        .headers()
        .get(CONTENT_TYPE)
        .is_some_and(is_application_json);
    if is_json {
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
    }
    response
}

fn is_application_json(value: &HeaderValue) -> bool {
    value
        .to_str()
        .map(|content_type| {
            content_type
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
        .unwrap_or(false)
}

const SLOW_REQUEST_LOG_THRESHOLD: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestLogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

async fn log_request(
    State(metrics_enabled): State<bool>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("")
        .to_owned();
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let span = info_span!(
        "request",
        method = %method,
        route = %route,
        path = %path,
        request_id = %request_id,
    );

    async move {
        let started_at = Instant::now();
        let request_metrics = (metrics_enabled
            && should_record_http_metrics(&method, &route, &path))
        .then(|| HttpRequestMetrics::start(&method, &route));
        let response = next.run(request).await;
        let latency = started_at.elapsed();
        if let Some(request_metrics) = request_metrics {
            request_metrics.finish(response.status(), latency);
        }
        log_request_end(response.status(), latency, &method, &route, &path);
        response
    }
    .instrument(span)
    .await
}

fn should_record_http_metrics(method: &Method, route: &str, path: &str) -> bool {
    if matches!(route, "/metrics" | "/health" | "/ready")
        || matches!(path, "/metrics" | "/health" | "/ready")
    {
        return false;
    }

    !(route.is_empty() && matches!(*method, Method::GET | Method::HEAD) && !is_backend_path(path))
}

fn is_backend_path(path: &str) -> bool {
    matches!(path, "/api" | "/mcp" | "/auth" | "/.well-known")
        || path.starts_with("/api/")
        || path.starts_with("/mcp/")
        || path.starts_with("/auth/")
        || path.starts_with("/.well-known/")
}

fn log_request_end(
    status: StatusCode,
    latency: Duration,
    method: &Method,
    route: &str,
    path: &str,
) {
    let status = status.as_u16();
    let latency_ms = latency.as_millis() as u64;
    match classify_request_log(status, latency, method, route, path) {
        RequestLogLevel::Debug => debug!(event = "request.end", status, latency_ms),
        RequestLogLevel::Info => info!(event = "request.end", status, latency_ms),
        RequestLogLevel::Warn => warn!(event = "request.end", status, latency_ms),
        RequestLogLevel::Error => error!(event = "request.end", status, latency_ms),
    }
}

fn classify_request_log(
    status: u16,
    latency: Duration,
    method: &Method,
    route: &str,
    path: &str,
) -> RequestLogLevel {
    if status >= 500 {
        return RequestLogLevel::Error;
    }

    if is_browser_auth_probe(status, method, route, path) {
        return RequestLogLevel::Debug;
    }

    if status >= 400 {
        return RequestLogLevel::Warn;
    }

    if latency >= SLOW_REQUEST_LOG_THRESHOLD {
        return RequestLogLevel::Info;
    }

    if is_control_plane_probe(route, path) {
        return RequestLogLevel::Debug;
    }

    if is_mutating_method(method) || is_auth_flow(route, path) || is_mcp_request(route, path) {
        return RequestLogLevel::Info;
    }

    if is_static_or_spa_success(status, method, route, path) {
        return RequestLogLevel::Debug;
    }

    RequestLogLevel::Debug
}

fn is_control_plane_probe(route: &str, path: &str) -> bool {
    matches!(route, "/health" | "/ready") || matches!(path, "/health" | "/ready")
}

fn is_static_or_spa_success(status: u16, method: &Method, route: &str, path: &str) -> bool {
    status < 400
        && method == Method::GET
        && route.is_empty()
        && !path.starts_with("/api/")
        && !path.starts_with("/auth/")
        && path != "/mcp"
        && !path.starts_with("/.well-known/")
}

fn is_mutating_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn is_auth_flow(route: &str, path: &str) -> bool {
    route.starts_with("/auth/") || path.starts_with("/auth/")
}

fn is_mcp_request(route: &str, path: &str) -> bool {
    route == "/mcp" || path == "/mcp"
}

fn is_browser_auth_probe(status: u16, method: &Method, route: &str, path: &str) -> bool {
    status == StatusCode::UNAUTHORIZED.as_u16()
        && method == Method::GET
        && (route == "/api/v1/me" || path == "/api/v1/me")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use axum::http::header::CACHE_CONTROL;
    use axum::routing::{get, post};
    use tower::ServiceExt as _;

    use super::*;

    #[tokio::test]
    async fn web_fallback_serves_index_with_revalidation() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("notegate-web-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("index.html"), "<html>notegate</html>").unwrap();

        let app = with_web_fallback(Router::new(), Some(dir.to_str().unwrap()));
        for path in ["/", "/index.html", "/dashboard"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                response.headers().get(CACHE_CONTROL),
                Some(&HeaderValue::from_static("no-cache, must-revalidate")),
                "{path}"
            );
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert_eq!(&body[..], b"<html>notegate</html>", "{path}");
        }

        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn web_assets_are_immutable_and_missing_assets_do_not_use_spa_fallback() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("notegate-web-assets-{nonce}"));
        fs::create_dir_all(dir.join("assets")).unwrap();
        fs::write(dir.join("index.html"), "<html>notegate</html>").unwrap();
        fs::write(dir.join("assets/app-abc123.js"), "export default true;").unwrap();

        let app = with_web_fallback(Router::new(), Some(dir.to_str().unwrap()));
        let asset_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/assets/app-abc123.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset_response.status(), StatusCode::OK);
        assert_eq!(
            asset_response.headers().get(CACHE_CONTROL),
            Some(&HeaderValue::from_static(
                "public, max-age=31536000, immutable"
            ))
        );

        let missing_response = app
            .oneshot(
                Request::builder()
                    .uri("/assets/removed-chunk.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
        assert_eq!(missing_response.headers().get(CACHE_CONTROL), None);
        let missing_body = axum::body::to_bytes(missing_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_ne!(&missing_body[..], b"<html>notegate</html>");

        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn data_plane_limits_reject_oversized_request_body() {
        let app = apply_data_plane_limits(
            Router::new().route("/", post(|body: String| async move { body })),
            DataPlaneLimits {
                request_body_max_bytes: 4,
                ..DataPlaneLimits::default()
            },
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .body(Body::from("12345"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn data_plane_limits_return_429_when_rate_limited() {
        let app = apply_data_plane_limits(
            Router::new().route("/", get(|| async { "ok" })),
            DataPlaneLimits {
                rate_limits: HttpRateLimitsConfig {
                    ingress: HttpRateLimitConfig {
                        requests_per_second: 1,
                        burst: 1,
                    },
                    ..HttpRateLimitsConfig::default()
                },
                ..DataPlaneLimits::default()
            },
        );

        let first = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let second = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(second.headers().contains_key("retry-after"));
    }

    #[tokio::test]
    async fn browser_v1_limit_does_not_consume_auth_route_capacity() {
        let browser_v1 = apply_rate_limit(
            Router::new().route("/api", get(|| async { "api" })),
            HttpRateLimitConfig {
                requests_per_second: 1,
                burst: 1,
            },
        );
        let app = apply_data_plane_limits(
            Router::new()
                .merge(browser_v1)
                .route("/auth", get(|| async { "auth" })),
            DataPlaneLimits::default(),
        );

        let first_api = app
            .clone()
            .oneshot(Request::builder().uri("/api").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let second_api = app
            .clone()
            .oneshot(Request::builder().uri("/api").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let public = app
            .oneshot(Request::builder().uri("/auth").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(first_api.status(), StatusCode::OK);
        assert_eq!(second_api.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(public.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn browser_v1_public_v2_and_mcp_rate_limits_are_independent() {
        let limit = HttpRateLimitConfig {
            requests_per_second: 1,
            burst: 1,
        };
        let app = Router::new()
            .merge(apply_rate_limit(
                Router::new().nest(
                    "/api",
                    Router::new().route("/v1/ping", get(|| async { "v1" })),
                ),
                limit,
            ))
            .merge(apply_rate_limit(
                Router::new().nest(
                    "/api/v2",
                    Router::new().route("/ping", get(|| async { "v2" })),
                ),
                limit,
            ))
            .merge(apply_rate_limit(
                Router::new().route("/mcp", get(|| async { "mcp" })),
                limit,
            ));

        for path in ["/api/v1/ping", "/api/v2/ping", "/mcp"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }
        for path in ["/api/v1/ping", "/api/v2/ping", "/mcp"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS, "{path}");
        }
    }

    #[tokio::test]
    async fn ingress_rate_limit_is_shared_across_surfaces() {
        let app = apply_data_plane_limits(
            Router::new()
                .route("/api/v1/ping", get(|| async { "v1" }))
                .route("/api/v2/ping", get(|| async { "v2" }))
                .route("/mcp", get(|| async { "mcp" })),
            DataPlaneLimits {
                rate_limits: HttpRateLimitsConfig {
                    ingress: HttpRateLimitConfig {
                        requests_per_second: 1,
                        burst: 2,
                    },
                    ..HttpRateLimitsConfig::default()
                },
                ..DataPlaneLimits::default()
            },
        );

        for path in ["/api/v1/ping", "/api/v2/ping"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }

        let response = app
            .oneshot(Request::builder().uri("/mcp").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn access_log_classification_suppresses_probe_and_normal_get_noise() {
        assert_eq!(
            classify_request_log(
                200,
                Duration::from_millis(1),
                &Method::GET,
                "/health",
                "/health"
            ),
            RequestLogLevel::Debug,
        );
        assert_eq!(
            classify_request_log(
                200,
                Duration::from_millis(1),
                &Method::GET,
                "/ready",
                "/ready"
            ),
            RequestLogLevel::Debug,
        );
        assert_eq!(
            classify_request_log(
                200,
                Duration::from_millis(20),
                &Method::GET,
                "/api/v1/spaces",
                "/api/v1/spaces",
            ),
            RequestLogLevel::Debug,
        );
        assert_eq!(
            classify_request_log(
                200,
                Duration::from_millis(1),
                &Method::GET,
                "",
                "/assets/app.js"
            ),
            RequestLogLevel::Debug,
        );
    }

    #[test]
    fn access_log_classification_keeps_operationally_relevant_requests() {
        assert_eq!(
            classify_request_log(
                201,
                Duration::from_millis(20),
                &Method::POST,
                "/api/v1/spaces",
                "/api/v1/spaces",
            ),
            RequestLogLevel::Info,
        );
        assert_eq!(
            classify_request_log(
                200,
                Duration::from_millis(20),
                &Method::POST,
                "/mcp",
                "/mcp"
            ),
            RequestLogLevel::Info,
        );
        assert_eq!(
            classify_request_log(
                307,
                Duration::from_millis(20),
                &Method::GET,
                "/auth/login",
                "/auth/login",
            ),
            RequestLogLevel::Info,
        );
        assert_eq!(
            classify_request_log(
                307,
                Duration::from_millis(20),
                &Method::GET,
                "",
                "/auth/login",
            ),
            RequestLogLevel::Info,
        );
        assert_eq!(
            classify_request_log(
                200,
                Duration::from_secs(2),
                &Method::GET,
                "/api/v1/spaces",
                "/api/v1/spaces",
            ),
            RequestLogLevel::Info,
        );
    }

    #[test]
    fn access_log_classification_escalates_failures_but_not_browser_auth_probe() {
        assert_eq!(
            classify_request_log(
                401,
                Duration::from_millis(1),
                &Method::GET,
                "/api/v1/me",
                "/api/v1/me",
            ),
            RequestLogLevel::Debug,
        );
        assert_eq!(
            classify_request_log(
                404,
                Duration::from_millis(1),
                &Method::GET,
                "/api/v1/spaces/{space_id}",
                "/api/v1/spaces/missing",
            ),
            RequestLogLevel::Warn,
        );
        assert_eq!(
            classify_request_log(
                500,
                Duration::from_millis(1),
                &Method::GET,
                "/api/v1/spaces",
                "/api/v1/spaces",
            ),
            RequestLogLevel::Error,
        );
    }

    #[test]
    fn http_metrics_exclude_control_plane_and_web_fallback_requests() {
        for path in ["/metrics", "/health", "/ready"] {
            assert!(!should_record_http_metrics(&Method::GET, path, path));
        }
        assert!(!should_record_http_metrics(
            &Method::GET,
            "",
            "/assets/app.js"
        ));
        assert!(!should_record_http_metrics(&Method::HEAD, "", "/dashboard"));
    }

    #[test]
    fn http_metrics_include_backend_workloads_and_unmatched_api_requests() {
        for (method, route, path) in [
            (Method::GET, "/api/v1/me", "/api/v1/me"),
            (Method::POST, "/mcp", "/mcp"),
            (Method::GET, "/auth/login", "/auth/login"),
            (
                Method::GET,
                "/.well-known/oauth-protected-resource",
                "/.well-known/oauth-protected-resource",
            ),
            (Method::GET, "", "/api/v1/missing"),
            (Method::GET, "", "/mcp/missing"),
        ] {
            assert!(should_record_http_metrics(&method, route, path));
        }
    }

    #[tokio::test]
    async fn control_plane_timeout_does_not_rate_limit() {
        let app = apply_control_plane_limits(Router::new().route("/", get(|| async { "ok" })));

        for _ in 0..3 {
            let response = app
                .clone()
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}
