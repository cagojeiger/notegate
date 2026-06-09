//! Identity category: `GET /api/v1/me` and `DELETE /api/v1/me`.
//!
//! `GET` returns the authenticated account, optional user/agent detail, and
//! global non-workspace capabilities via the shared [`build_me`] builder, kept
//! aligned with the MCP `me` tool (`docs/spec/mcp/identity.md`). Workspace-specific
//! roles live in the Workspaces category, not in `/me`.
//!
//! `DELETE` is the user account teardown endpoint. It is intentionally REST-only:
//! MCP remains a file/workspace tool surface and does not expose account deletion.

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use notegate_model::Caller;

use crate::error::ApiError;
use crate::identity::me::{MeOutput, build_me};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/me", get(get_me).delete(delete_me))
}

#[utoipa::path(
    get,
    path = "/api/v1/me",
    tag = "identity",
    responses((status = 200, description = "Get current caller", body = MeOutput)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn get_me(Extension(caller): Extension<Caller>) -> Json<MeOutput> {
    Json(build_me(&caller))
}

#[utoipa::path(
    delete,
    path = "/api/v1/me",
    tag = "identity",
    responses((status = 204, description = "Delete current user account")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn delete_me(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<StatusCode, ApiError> {
    state
        .account_lifecycle
        .delete_me(caller.account.kind, caller.account_id())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
