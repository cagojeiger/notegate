//! History queries for the current user and authorized Space changes.
//! Storage and authorization remain in the corresponding services.

use axum::extract::{Extension, Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use notegate_model::{
    Caller, CommandInvocationSurface, ListAuditEvents, ListBackgroundJobs, ListCommandInvocations,
};
use notegate_service::files::{ListFileChangeEvents, SyncFileChanges};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::set_private_no_store;
use crate::error::ApiError;
use crate::page::Page;
use crate::rest::dto::{
    AuditEventListResponse, AuditEventOut, BackgroundJobDetailResponse, BackgroundJobListResponse,
    BackgroundJobOut, CommandInvocationListResponse, CommandInvocationOut, FileChangeDeltaOut,
    FileChangeEventListResponse, FileChangeEventOut, FileChangeSyncResponse,
};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/me/file-change-events", get(list_owned_file_changes))
        .route("/v1/me/audit-events", get(list_audit_events))
        .route("/v1/me/command-invocations", get(list_command_invocations))
        .route("/v1/me/jobs", get(list_background_jobs))
        .route("/v1/me/jobs/{job_id}", get(get_background_job))
        .route(
            "/v1/spaces/{space_id}/file-change-events",
            get(list_file_change_events),
        )
        .route(
            "/v1/spaces/{space_id}/file-change-sync",
            get(sync_file_changes),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListEventsQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListCommandInvocationsQuery {
    surface: CommandInvocationSurface,
    limit: Option<i64>,
    cursor: Option<String>,
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
        .history
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
    path = "/api/v1/me/command-invocations",
    tag = "events",
    params(
        ("surface" = String, Query, description = "Invocation surface: mcp or cli"),
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List current user's external command invocation history", body = CommandInvocationListResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn list_command_invocations(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListCommandInvocationsQuery>,
) -> Result<Response, ApiError> {
    let page = state
        .history
        .list_command_invocations(
            caller.account.kind,
            caller.account_id(),
            ListCommandInvocations {
                surface: query.surface,
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
    let command_invocations = page
        .items
        .iter()
        .map(|invocation| CommandInvocationOut::from_invocation(invocation, &refs))
        .collect();
    let mut response = Json(CommandInvocationListResponse {
        command_invocations,
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
        .history
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
        .history
        .get_background_job(caller.account.kind, caller.account_id(), job_id)
        .await?;
    let mut response = Json(BackgroundJobDetailResponse::from(&detail)).into_response();
    set_private_no_store(&mut response);
    Ok(response)
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListFileChangeEventsQuery {
    node_id: Option<Uuid>,
    limit: Option<i64>,
    cursor: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/file-change-events",
    tag = "events",
    params(
        ("space_id" = Uuid, Path),
        ("node_id" = Option<Uuid>, Query, description = "Optional node id filter"),
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List file change event history in a space", body = FileChangeEventListResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn list_file_change_events(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Query(query): Query<ListFileChangeEventsQuery>,
) -> Result<Json<FileChangeEventListResponse>, ApiError> {
    let page = state
        .files
        .list_file_change_events(
            caller.account_id(),
            space_id,
            ListFileChangeEvents {
                node_id: query.node_id,
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
        .map(|event| FileChangeEventOut::from_event(event, &refs))
        .collect();

    Ok(Json(FileChangeEventListResponse {
        events,
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct OwnedChangesQuery {
    space_id: Option<Uuid>,
    limit: Option<i64>,
    cursor: Option<String>,
}

#[utoipa::path(
    get, path = "/api/v1/me/file-change-events", tag = "events",
    params(("space_id" = Option<Uuid>, Query), ("limit" = Option<i64>, Query), ("cursor" = Option<String>, Query)),
    responses((status = 200, description = "Owned change snapshots, including removed Spaces", body = FileChangeEventListResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn list_owned_file_changes(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<OwnedChangesQuery>,
) -> Result<Response, ApiError> {
    let page = state
        .history
        .list_file_changes(
            caller.account.kind,
            caller.account_id(),
            query.space_id,
            query.limit,
            query.cursor,
        )
        .await?;
    let ids: Vec<_> = page
        .items
        .iter()
        .filter_map(|e| e.actor_account_id)
        .collect();
    let refs = state.accounts.find_account_refs(&ids).await?;
    let mut response = Json(FileChangeEventListResponse {
        events: page
            .items
            .iter()
            .map(|e| FileChangeEventOut::from_event(e, &refs))
            .collect(),
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    })
    .into_response();
    set_private_no_store(&mut response);
    Ok(response)
}

#[derive(Debug, Deserialize)]
pub(crate) struct SyncFileChangesQuery {
    after_id: Option<i64>,
    limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/file-change-sync",
    tag = "events",
    params(
        ("space_id" = Uuid, Path),
        ("after_id" = Option<i64>, Query, description = "Last applied event id; omit to establish a baseline"),
        ("limit" = Option<i64>, Query, description = "Page size"),
    ),
    responses((status = 200, description = "Read file changes after a sync token", body = FileChangeSyncResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn sync_file_changes(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Query(query): Query<SyncFileChangesQuery>,
) -> Result<Json<FileChangeSyncResponse>, ApiError> {
    let page = state
        .files
        .sync_file_changes(
            caller.account_id(),
            space_id,
            SyncFileChanges {
                after_id: query.after_id,
                limit: query.limit,
            },
        )
        .await?;
    Ok(Json(FileChangeSyncResponse {
        changes: page
            .items
            .iter()
            .map(FileChangeDeltaOut::from_event)
            .collect(),
        next_after_id: page.next_after_id,
        has_more: page.has_more,
        resync_required: page.resync_required,
    }))
}
