//! Identity category: `GET /api/v1/me`.
//!
//! Returns the authenticated account, optional user/agent detail, and global
//! non-workspace capabilities via the shared [`build_me`] builder, kept aligned
//! with the MCP `me` tool (`docs/spec/mcp/identity.md`). Workspace-specific roles live
//! in the Workspaces category, not in `/me`.

use axum::extract::Extension;
use axum::routing::get;
use axum::{Json, Router};
use notegate_model::Caller;

use crate::identity::me::{MeOutput, build_me};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/me", get(get_me))
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
