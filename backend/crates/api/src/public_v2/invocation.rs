//! Authenticated v2 request/response summaries, without buffering either body.

use std::collections::BTreeMap;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{Extension, FromRequestParts, MatchedPath, Path, State};
use axum::http::{Method, Request};
use axum::middleware::Next;
use axum::response::Response;
use notegate_model::{Caller, files::FileMutationContext};
use serde_json::json;
use uuid::Uuid;

use crate::invocations::{InvocationRecord, InvocationSurface, record};
use crate::observability::CommandInvocationMetrics;
use crate::state::AppState;

pub(super) async fn capture(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let started = Instant::now();
    let metrics = CommandInvocationMetrics::start(state.config.metrics_enabled, "api", "http");
    let invocation_id = Uuid::new_v4();
    let method = match *request.method() {
        Method::GET => "GET",
        Method::HEAD => "HEAD",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::OPTIONS => "OPTIONS",
        _ => "OTHER",
    };
    // MatchedPath is a server-defined template, never a raw URL or query string.
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    let operation = format!("{method} {route}");
    let (mut parts, body) = request.into_parts();
    let path_ids: BTreeMap<String, Uuid> =
        Path::<BTreeMap<String, String>>::from_request_parts(&mut parts, &state)
            .await
            .map(|Path(params)| {
                params
                    .into_iter()
                    .filter(|(key, _)| matches!(key.as_str(), "space_id" | "node_id" | "upload_id"))
                    .filter_map(|(key, value)| Uuid::parse_str(&value).ok().map(|id| (key, id)))
                    .collect()
            })
            .unwrap_or_default();
    parts.extensions.insert(FileMutationContext {
        invocation_id: Some(invocation_id),
        ..FileMutationContext::for_channel(caller.channel)
    });
    let response = next.run(Request::from_parts(parts, body)).await;
    let elapsed = started.elapsed();
    let status = response.status();
    let error_code = (status.is_client_error() || status.is_server_error())
        .then(|| format!("http_{}", status.as_u16()));
    let outcome = if error_code.is_some() {
        "error"
    } else {
        "success"
    };
    metrics.execution_finished(outcome, elapsed);

    let history_started = Instant::now();
    let input =
        json!({"method": method, "route": route, "path_ids": path_ids, "body_recorded": false});
    let snapshot = json!({"status": status.as_u16(), "body_recorded": false});
    let recorded = record(
        &state,
        &caller,
        InvocationRecord {
            id: Some(invocation_id),
            surface: InvocationSurface::Api,
            tool: "http",
            op: Some(&operation),
            purpose: None,
            space_name: None,
            input: &input,
            response: Some(&snapshot),
            error_code: error_code.as_deref(),
            elapsed_ms: elapsed.as_millis(),
        },
    )
    .await;
    metrics.history_finished(recorded, history_started.elapsed());
    metrics.finish(outcome, started.elapsed());
    response
}
