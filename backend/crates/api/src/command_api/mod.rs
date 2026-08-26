//! Thin HTTP boundary for the transport-neutral command engine.

mod context;
mod error;
mod invocation;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::Value;

use self::context::HttpCommandContext;
use self::error::CommandHttpError;
use crate::state::AppState;

/// The unversioned CLI transport. Command schemas use an explicit compatibility
/// protocol and are shared with MCP through `notegate-command`.
pub(crate) fn routes() -> Router<AppState> {
    Router::new().route("/cli", post(invoke))
}

async fn invoke(
    State(state): State<AppState>,
    context: HttpCommandContext,
    headers: HeaderMap,
    input: invocation::RawJsonInput,
) -> Result<Json<Value>, CommandHttpError> {
    invocation::execute(state, context, &headers, input).await
}

#[cfg(test)]
mod tests;
