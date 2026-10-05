//! Identity category: current caller, user usage, and deletion.
//!
//! `GET` returns the authenticated account, optional user/agent detail, and
//! global non-space capabilities via the shared [`build_me`] builder, kept
//! aligned with the MCP `me` tool (`docs/spec/mcp/identity.md`). Space-specific
//! permissions live in the Spaces category, not in `/me`.
//!
//! `DELETE` is the user account teardown endpoint. It is intentionally REST-only:
//! MCP remains a file/space tool surface and does not expose account deletion.

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use notegate_model::Caller;
use notegate_service::usage::{CurrentUserUsage, QuotaUsage};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::identity::me::{MeOutput, build_me};
use crate::rest::dto::CommandAvailability;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/me", get(get_me).delete(delete_me))
        .route("/v1/me/usage", get(get_usage))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct QuotaUsageOut {
    used: usize,
    limit: usize,
}

impl From<QuotaUsage> for QuotaUsageOut {
    fn from(value: QuotaUsage) -> Self {
        Self {
            used: value.used,
            limit: value.limit,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SpaceUsageOut {
    id: Uuid,
    name: String,
    items: QuotaUsageOut,
    text_bytes: QuotaUsageOut,
    file_bytes: QuotaUsageOut,
    reconciliation: UsageReconciliationStatusOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct UsageReconciliationStatusOut {
    status: UsageReconciliationStatus,
    availability: CommandAvailability,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UsageReconciliationStatus {
    Idle,
    Pending,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CurrentUserUsageOut {
    tier: String,
    spaces: Vec<SpaceUsageOut>,
}

impl From<CurrentUserUsage> for CurrentUserUsageOut {
    fn from(value: CurrentUserUsage) -> Self {
        let now = Utc::now();
        Self {
            tier: value.tier.as_str().to_owned(),
            spaces: value
                .spaces
                .into_iter()
                .map(|space| {
                    let availability = if space.reconciliation_pending {
                        CommandAvailability::pending()
                    } else if space.reconciliation_available_at > now {
                        CommandAvailability::cooldown(space.reconciliation_available_at)
                    } else {
                        CommandAvailability::available()
                    };
                    SpaceUsageOut {
                        id: space.id,
                        name: space.name,
                        items: space.items.into(),
                        text_bytes: space.text_bytes.into(),
                        file_bytes: space.file_bytes.into(),
                        reconciliation: UsageReconciliationStatusOut {
                            status: if space.reconciliation_pending {
                                UsageReconciliationStatus::Pending
                            } else {
                                UsageReconciliationStatus::Idle
                            },
                            availability,
                        },
                    }
                })
                .collect(),
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/me",
    tag = "identity",
    responses((status = 200, description = "Get current caller", body = MeOutput)),
    security(("browser_session" = []))
)]
pub(crate) async fn get_me(Extension(caller): Extension<Caller>) -> Json<MeOutput> {
    Json(build_me(&caller))
}

#[utoipa::path(
    get,
    path = "/api/v1/me/usage",
    tag = "identity",
    responses((status = 200, description = "Get current user's Space usage", body = CurrentUserUsageOut)),
    security(("browser_session" = []))
)]
pub(crate) async fn get_usage(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<CurrentUserUsageOut>, ApiError> {
    let usage = state
        .usage
        .current_user(caller.account.kind, caller.account_id())
        .await?;
    Ok(Json(usage.into()))
}

#[utoipa::path(
    delete,
    path = "/api/v1/me",
    tag = "identity",
    responses((status = 204, description = "Delete current user account")),
    security(("browser_session" = []))
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
