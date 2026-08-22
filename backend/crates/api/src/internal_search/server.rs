use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use notegate_db::PgPool;
use notegate_search::{SearchRunError, SearchRuntime};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tower::ServiceBuilder;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

use super::auth::{
    InternalSearchAuth, REQUEST_SIGNATURE_HEADER, RESPONSE_SIGNATURE_HEADER, TIMESTAMP_HEADER,
};
use super::contract::{
    ErrorOutput, FindCommand, FindOutput, GrepCommand, GrepOutput, InternalSearchError,
};
use super::{FIND_PATH, GREP_PATH};
use crate::error::ApiError;
use crate::observability::{self, MetricsHandle};

const MAX_REQUEST_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) struct SearchServerState {
    pub(crate) authority_db: PgPool,
    authority_db_max_connections: u32,
    read_db: Option<(PgPool, u32)>,
    runtime: SearchRuntime,
    auth: InternalSearchAuth,
    metrics: Option<MetricsHandle>,
}

impl SearchServerState {
    pub(crate) fn new(
        authority_db: PgPool,
        authority_db_max_connections: u32,
        read_db: Option<(PgPool, u32)>,
        runtime: SearchRuntime,
        signing_key: [u8; 32],
        metrics: Option<MetricsHandle>,
    ) -> Self {
        Self {
            authority_db,
            authority_db_max_connections,
            read_db,
            runtime,
            auth: InternalSearchAuth::new(signing_key),
            metrics,
        }
    }

    pub(crate) const fn metrics_enabled(&self) -> bool {
        self.metrics.is_some()
    }
}

pub(crate) fn routes(metrics_enabled: bool) -> Router<SearchServerState> {
    let router = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route(FIND_PATH, post(find))
        .route(GREP_PATH, post(grep));
    let router = if metrics_enabled {
        router.route("/metrics", get(scrape))
    } else {
        router
    };
    router.layer(
        ServiceBuilder::new()
            .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BYTES))
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                std::time::Duration::from_secs(notegate_core::limits::HTTP_REQUEST_TIMEOUT_SECS),
            )),
    )
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn ready(State(state): State<SearchServerState>) -> Result<Json<HealthResponse>, ApiError> {
    check_database_ready("primary", &state.authority_db).await?;
    if let Some((read_db, _max_connections)) = &state.read_db {
        check_database_ready("read", read_db).await?;
    }
    Ok(Json(HealthResponse { status: "ready" }))
}

async fn check_database_ready(role: &'static str, pool: &PgPool) -> Result<(), ApiError> {
    notegate_db::check_readiness(pool).await.map_err(|error| {
        tracing::error!(event = "search.ready.failed", database_role = role, %error);
        ApiError::internal("database not ready")
    })
}

async fn scrape(State(state): State<SearchServerState>) -> Response {
    let Some(metrics) = &state.metrics else {
        return StatusCode::NOT_FOUND.into_response();
    };
    observability::record_database_metrics(
        "primary",
        &state.authority_db,
        state.authority_db_max_connections,
    );
    if let Some((read_db, max_connections)) = &state.read_db {
        observability::record_database_metrics("read", read_db, *max_connections);
    }
    state.runtime.record_body_cache_metrics();
    observability::scrape_response(metrics)
}

async fn find(State(state): State<SearchServerState>, request: Request<Body>) -> Response {
    let (timestamp, command) = match verified_json::<FindCommand>(&state, request).await {
        Ok(value) => value,
        Err(error) => return request_error_response(&state, FIND_PATH, error),
    };
    let caller_account_id = command.caller_account_id;
    let space_id = command.space_id;
    let result = state
        .runtime
        .find(caller_account_id, space_id, command.into_request())
        .await
        .map(FindOutput::from);
    search_response(&state, FIND_PATH, timestamp, result)
}

async fn grep(State(state): State<SearchServerState>, request: Request<Body>) -> Response {
    let (timestamp, command) = match verified_json::<GrepCommand>(&state, request).await {
        Ok(value) => value,
        Err(error) => return request_error_response(&state, GREP_PATH, error),
    };
    let caller_account_id = command.caller_account_id;
    let space_id = command.space_id;
    let result = state
        .runtime
        .grep(caller_account_id, space_id, command.into_request())
        .await
        .map(GrepOutput::from);
    search_response(&state, GREP_PATH, timestamp, result)
}

enum VerifiedRequestError {
    Unauthorized,
    InvalidJson { timestamp: i64 },
    TooLarge,
}

async fn verified_json<T>(
    state: &SearchServerState,
    request: Request<Body>,
) -> Result<(i64, T), VerifiedRequestError>
where
    T: DeserializeOwned,
{
    let (parts, body) = request.into_parts();
    let timestamp = parts
        .headers
        .get(TIMESTAMP_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or(VerifiedRequestError::Unauthorized)?;
    let signature = parts
        .headers
        .get(REQUEST_SIGNATURE_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or(VerifiedRequestError::Unauthorized)?;
    let body = to_bytes(body, MAX_REQUEST_BYTES)
        .await
        .map_err(|_error| VerifiedRequestError::TooLarge)?;
    let now =
        InternalSearchAuth::now_timestamp().map_err(|_error| VerifiedRequestError::Unauthorized)?;
    if !state.auth.verify_request_at(
        now,
        timestamp,
        parts.method.as_str(),
        parts.uri.path(),
        &body,
        signature,
    ) {
        return Err(VerifiedRequestError::Unauthorized);
    }
    let input = serde_json::from_slice(&body)
        .map_err(|_error| VerifiedRequestError::InvalidJson { timestamp })?;
    Ok((timestamp, input))
}

fn request_error_response(
    state: &SearchServerState,
    path: &str,
    error: VerifiedRequestError,
) -> Response {
    match error {
        VerifiedRequestError::Unauthorized => StatusCode::NOT_FOUND.into_response(),
        VerifiedRequestError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        VerifiedRequestError::InvalidJson { timestamp } => signed_json(
            state,
            path,
            timestamp,
            StatusCode::BAD_REQUEST,
            &ErrorOutput {
                error: InternalSearchError::InvalidInput {
                    message: "invalid internal search request".to_owned(),
                },
            },
        ),
    }
}

fn search_response<T>(
    state: &SearchServerState,
    path: &str,
    timestamp: i64,
    result: Result<T, SearchRunError>,
) -> Response
where
    T: Serialize,
{
    match result {
        Ok(output) => signed_json(state, path, timestamp, StatusCode::OK, &output),
        Err(SearchRunError::Capacity(capacity)) => signed_json(
            state,
            path,
            timestamp,
            StatusCode::TOO_MANY_REQUESTS,
            &ErrorOutput {
                error: InternalSearchError::busy(capacity),
            },
        ),
        Err(SearchRunError::Search(error)) => {
            let status = search_error_status(&error);
            if let notegate_search::SearchError::Internal(detail) = &error {
                tracing::error!(event = "internal_search.execution_failed", %detail);
            }
            signed_json(
                state,
                path,
                timestamp,
                status,
                &ErrorOutput {
                    error: InternalSearchError::from_search(error),
                },
            )
        }
    }
}

fn search_error_status(error: &notegate_search::SearchError) -> StatusCode {
    match error {
        notegate_search::SearchError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        notegate_search::SearchError::NotFound(_) => StatusCode::NOT_FOUND,
        notegate_search::SearchError::Forbidden(_) => StatusCode::FORBIDDEN,
        notegate_search::SearchError::Conflict(_)
        | notegate_search::SearchError::WriteLocked { .. }
        | notegate_search::SearchError::UsageRecalculationInProgress { .. } => StatusCode::CONFLICT,
        notegate_search::SearchError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn signed_json<T>(
    state: &SearchServerState,
    path: &str,
    timestamp: i64,
    status: StatusCode,
    output: &T,
) -> Response
where
    T: Serialize,
{
    let Ok(body) = serde_json::to_vec(output) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(signature) = state
        .auth
        .sign_response(timestamp, status.as_u16(), path, &body)
    else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(timestamp) = HeaderValue::from_str(&timestamp.to_string()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(signature) = HeaderValue::from_str(&signature) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = (status, body).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response.headers_mut().insert(TIMESTAMP_HEADER, timestamp);
    response
        .headers_mut()
        .insert(RESPONSE_SIGNATURE_HEADER, signature);
    response
}
