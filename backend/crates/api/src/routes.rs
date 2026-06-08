//! Router assembly and HTTP handlers.

use std::time::Duration;

use axum::body::Body;
use axum::extract::{MatchedPath, State};
use axum::http::header::{CONTENT_TYPE, HeaderName};
use axum::http::{HeaderValue, Request};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::Response;
use axum::routing::{any, get};
use axum::{Json, Router};
use serde::Serialize;
use tower::ServiceBuilder;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
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
        .merge(system_routes())
        .merge(auth_routes())
        .merge(metadata_routes(&state))
        .merge(crate::openapi::routes(&state.config))
        .nest("/api", rest_api_routes(state.clone()))
        .route("/mcp", any(mcp_handler))
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
        .route("/auth/logout", get(logout))
        // Compatibility aliases: the current authgate `notegate-web` client is
        // registered with `http://localhost:9191/callback`. Keep these until the
        // external client registration moves to `/auth/callback`.
        .route("/login", get(login))
        .route("/callback", get(callback))
        .route("/success", get(success))
        .route("/logout", get(logout))
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
        .merge(crate::rest::workspaces::routes())
        .merge(crate::rest::nodes::routes())
        .merge(crate::rest::documents::routes())
        .merge(crate::rest::search::routes())
        .merge(crate::rest::access::routes())
        .merge(crate::rest::agents::routes())
        .fallback(api_not_found)
        .layer(from_fn_with_state(state, require_bearer))
}

/// Liveness: the process is up. No dependency checks.
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Readiness: verify the database is reachable before reporting ready.
async fn ready(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map_err(|error| {
            tracing::error!(event = "ready.db_unreachable", %error);
            ApiError::internal("database unreachable")
        })?;

    Ok(Json(HealthResponse { status: "ready" }))
}

async fn api_not_found() -> axum::http::StatusCode {
    axum::http::StatusCode::NOT_FOUND
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

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}
