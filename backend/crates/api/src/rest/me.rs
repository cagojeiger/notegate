//! Identity category: current caller, user usage, events, and deletion.
//!
//! `GET` returns the authenticated account, optional user/agent detail, and
//! global non-space capabilities via the shared [`build_me`] builder, kept
//! aligned with the MCP `me` tool (`docs/spec/mcp/identity.md`). Space-specific
//! permissions live in the Spaces category, not in `/me`.
//!
//! `DELETE` is the user account teardown endpoint. It is intentionally REST-only:
//! MCP remains a file/space tool surface and does not expose account deletion.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use notegate_model::{Caller, ListAuditEvents, ListBackgroundJobs, ListMcpInvocations};
use notegate_service::usage::{CurrentUserUsage, QuotaUsage};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::set_private_no_store;
use crate::error::ApiError;
use crate::identity::me::{MeOutput, build_me};
use crate::page::Page;
use crate::rest::dto::{
    AuditEventListResponse, AuditEventOut, BackgroundJobDetailResponse, BackgroundJobListResponse,
    BackgroundJobOut, McpInvocationListResponse, McpInvocationOut,
};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/me", get(get_me).delete(delete_me))
        .route("/v1/me/usage", get(get_usage))
        .route("/v1/me/audit-events", get(list_audit_events))
        .route("/v1/me/mcp-invocations", get(list_mcp_invocations))
        .route("/v1/me/jobs", get(list_background_jobs))
        .route("/v1/me/jobs/{job_id}", get(get_background_job))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListEventsQuery {
    limit: Option<i64>,
    cursor: Option<String>,
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
    reconciliation_pending: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CurrentUserUsageOut {
    tier: String,
    spaces: Vec<SpaceUsageOut>,
}

impl From<CurrentUserUsage> for CurrentUserUsageOut {
    fn from(value: CurrentUserUsage) -> Self {
        Self {
            tier: value.tier.as_str().to_owned(),
            spaces: value
                .spaces
                .into_iter()
                .map(|space| SpaceUsageOut {
                    id: space.id,
                    name: space.name,
                    items: space.items.into(),
                    text_bytes: space.text_bytes.into(),
                    file_bytes: space.file_bytes.into(),
                    reconciliation_pending: space.reconciliation_pending,
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
    get,
    path = "/api/v1/me/audit-events",
    tag = "events",
    params(
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List current user audit event history", body = AuditEventListResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn list_audit_events(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListEventsQuery>,
) -> Result<Json<AuditEventListResponse>, ApiError> {
    let page = state
        .account_lifecycle
        .list_audit_events(
            caller.account.kind,
            caller.account_id(),
            ListAuditEvents {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let actor_ids = page
        .items
        .iter()
        .filter_map(|event| event.actor_account_id)
        .collect::<Vec<_>>();
    let refs = state.accounts.find_account_refs(&actor_ids).await?;
    let events = page
        .items
        .iter()
        .map(|event| AuditEventOut::from_event(event, &refs))
        .collect();
    Ok(Json(AuditEventListResponse {
        events,
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/me/mcp-invocations",
    tag = "events",
    params(
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List current user's MCP invocation history", body = McpInvocationListResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn list_mcp_invocations(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListEventsQuery>,
) -> Result<Response, ApiError> {
    let page = state
        .account_lifecycle
        .list_mcp_invocations(
            caller.account.kind,
            caller.account_id(),
            ListMcpInvocations {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let actor_ids = page
        .items
        .iter()
        .map(|invocation| invocation.actor_account_id)
        .collect::<Vec<_>>();
    let refs = state.accounts.find_account_refs(&actor_ids).await?;
    let invocations = page
        .items
        .iter()
        .map(|invocation| McpInvocationOut::from_invocation(invocation, &refs))
        .collect();
    let mut response = Json(McpInvocationListResponse {
        invocations,
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    })
    .into_response();
    set_private_no_store(&mut response);
    Ok(response)
}

#[utoipa::path(
    get,
    path = "/api/v1/me/jobs",
    tag = "events",
    params(
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List current user's background job history", body = BackgroundJobListResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn list_background_jobs(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListEventsQuery>,
) -> Result<Response, ApiError> {
    let page = state
        .account_lifecycle
        .list_background_jobs(
            caller.account.kind,
            caller.account_id(),
            ListBackgroundJobs {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let jobs = page.items.iter().map(BackgroundJobOut::from).collect();
    let mut response = Json(BackgroundJobListResponse {
        jobs,
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    })
    .into_response();
    set_private_no_store(&mut response);
    Ok(response)
}

#[utoipa::path(
    get,
    path = "/api/v1/me/jobs/{job_id}",
    tag = "events",
    params(("job_id" = Uuid, Path, description = "Background job id")),
    responses(
        (status = 200, description = "Get a background job and its attempts", body = BackgroundJobDetailResponse),
        (status = 404, description = "Background job not found", body = crate::error::ErrorResponse),
    ),
    security(("browser_session" = []))
)]
pub(crate) async fn get_background_job(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(job_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let detail = state
        .account_lifecycle
        .get_background_job(caller.account.kind, caller.account_id(), job_id)
        .await?;
    let mut response = Json(BackgroundJobDetailResponse::from(&detail)).into_response();
    set_private_no_store(&mut response);
    Ok(response)
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
