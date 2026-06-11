//! Router assembly and HTTP handlers.

use std::time::Duration;

use axum::body::Body;
use axum::extract::{MatchedPath, State};
use axum::http::header::{CONTENT_TYPE, HeaderName};
use axum::http::{HeaderValue, Request, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::Response;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use axum_governor::extractor::Global;
use axum_governor::{GovernorConfigBuilder, GovernorLayer, Quota};
use notegate_core::limits;
use serde::Serialize;
use tower::ServiceBuilder;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing::{Span, info, info_span};

use crate::auth::bearer::require_bearer;
use crate::auth::metadata::{
    authorization_server_metadata, protected_resource_metadata, protected_resource_metadata_url,
};
use crate::auth::oauth::{callback, login, logout, success};
use crate::error::ApiError;
use crate::mcp::server::mcp_handler;
use crate::state::AppState;

pub fn app(state: AppState) -> Router {
    let x_request_id = HeaderName::from_static("x-request-id");

    Router::new()
        .merge(control_plane_routes())
        .merge(data_plane_routes(state.clone()))
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::new(
                    x_request_id.clone(),
                    MakeRequestUuid,
                ))
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(make_request_span)
                        .on_response(log_request_end),
                )
                .layer(from_fn(add_json_charset))
                .layer(PropagateRequestIdLayer::new(x_request_id)),
        )
}

#[derive(Debug, Clone, Copy)]
struct DataPlaneLimits {
    request_body_max_bytes: usize,
    request_timeout: Duration,
    rate_limit_requests_per_minute: u32,
}

impl Default for DataPlaneLimits {
    fn default() -> Self {
        Self {
            request_body_max_bytes: limits::HTTP_REQUEST_BODY_MAX_BYTES,
            request_timeout: Duration::from_secs(limits::HTTP_REQUEST_TIMEOUT_SECS),
            rate_limit_requests_per_minute: limits::HTTP_RATE_LIMIT_REQUESTS_PER_MINUTE,
        }
    }
}

fn control_plane_routes() -> Router<AppState> {
    apply_control_plane_limits(system_routes())
}

fn data_plane_routes(state: AppState) -> Router<AppState> {
    let router = Router::new()
        .merge(auth_routes())
        .merge(metadata_routes(&state))
        .merge(crate::openapi::routes(&state.config))
        .nest("/api", rest_api_routes(state))
        .route("/mcp", any(mcp_handler));
    apply_data_plane_limits(router, DataPlaneLimits::default())
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
            .layer(GovernorLayer::new(rate_limit_config(limits))),
    )
}

#[allow(clippy::expect_used)]
fn rate_limit_config(limits: DataPlaneLimits) -> axum_governor::GovernorConfig<()> {
    let requests = std::num::NonZeroU32::new(limits.rate_limit_requests_per_minute)
        .expect("HTTP rate limit must be greater than zero");
    GovernorConfigBuilder::default()
        .with_extractor(Global)
        .quota_default(Quota::requests_per_minute(requests))
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
        .merge(crate::rest::files::routes())
        .merge(crate::rest::connections::routes())
        .merge(crate::rest::agents::routes())
        .fallback(api_not_found)
        .layer(from_fn_with_state(state, require_bearer))
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

fn log_request_end<B>(response: &axum::http::Response<B>, latency: Duration, _span: &Span) {
    info!(
        event = "request.end",
        status = response.status().as_u16(),
        latency_ms = latency.as_millis() as u64,
    );
}

fn make_request_span<B>(req: &Request<B>) -> Span {
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("");
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    info_span!(
        "request",
        method = %req.method(),
        route,
        request_id,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use axum::routing::{get, post};
    use tower::ServiceExt as _;

    use super::*;

    #[tokio::test]
    async fn data_plane_limits_reject_oversized_request_body() {
        let app = apply_data_plane_limits(
            Router::new().route("/", post(|body: String| async move { body })),
            DataPlaneLimits {
                request_body_max_bytes: 4,
                request_timeout: Duration::from_secs(30),
                rate_limit_requests_per_minute: 100,
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
                request_body_max_bytes: 1024,
                request_timeout: Duration::from_secs(30),
                rate_limit_requests_per_minute: 1,
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
